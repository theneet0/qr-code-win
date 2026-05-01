#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

use eframe::egui;
use image::{GrayImage, Luma};
use nokhwa::{
    pixel_format::RgbFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType},
    Camera,
};
use qrcode::QrCode;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([620.0, 460.0]),
        ..Default::default()
    };

    eframe::run_native(
        "QR Studio",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(QrApp::default()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Generate,
    Scan,
}

enum ScanMessage {
    Found(String),
    Status(String),
    Error(String),
}

struct ScannerHandle {
    stop: Arc<AtomicBool>,
    rx: mpsc::Receiver<ScanMessage>,
    join: Option<thread::JoinHandle<()>>,
}

struct QrApp {
    tab: Tab,
    qr_input: String,
    qr_texture: Option<egui::TextureHandle>,
    qr_image: Option<GrayImage>,
    generator_status: String,
    scanner: Option<ScannerHandle>,
    scanner_status: String,
    scanned_text: String,
}

impl Default for QrApp {
    fn default() -> Self {
        Self {
            tab: Tab::Generate,
            qr_input: "https://example.com".to_owned(),
            qr_texture: None,
            qr_image: None,
            generator_status: "متن یا لینک را وارد کن و Generate را بزن.".to_owned(),
            scanner: None,
            scanner_status: "اسکنر خاموش است.".to_owned(),
            scanned_text: String::new(),
        }
    }
}

impl Drop for QrApp {
    fn drop(&mut self) {
        self.stop_scanner();
    }
}

impl eframe::App for QrApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scanner_messages();

        let ctx = ui.ctx().clone();

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("QR Studio - Rust / Windows 11");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Generate, "ساخت QR");
                ui.selectable_value(&mut self.tab, Tab::Scan, "خواندن با دوربین");
            });

            ui.separator();

            match self.tab {
                Tab::Generate => self.generator_ui(ui, &ctx),
                Tab::Scan => self.scanner_ui(ui),
            }
        });

        if self.scanner.is_some() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

impl QrApp {
    fn generator_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("متن، لینک، کانفیگ یا هر چیزی که می‌خواهی QR شود:");
        ui.add(
            egui::TextEdit::multiline(&mut self.qr_input)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Generate").clicked() {
                self.generate_qr(ctx);
            }

            if ui.button("Save PNG").clicked() {
                self.save_qr_png();
            }
        });

        ui.add_space(10.0);
        ui.label(&self.generator_status);

        if let Some(texture) = &self.qr_texture {
            ui.add_space(10.0);
            ui.image((texture.id(), egui::vec2(320.0, 320.0)));
        }
    }

    fn scanner_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("این بخش از دوربین پیش‌فرض لپ‌تاپ استفاده می‌کند. QR را جلوی دوربین بگیر.");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if self.scanner.is_none() {
                if ui.button("Start Scan").clicked() {
                    self.start_scanner();
                }
            } else if ui.button("Stop Scan").clicked() {
                self.stop_scanner();
            }

            if ui.button("Clear Result").clicked() {
                self.scanned_text.clear();
            }
        });

        ui.add_space(10.0);
        ui.label(format!("وضعیت: {}", self.scanner_status));
        ui.separator();
        ui.label("نتیجه خوانده‌شده:");

        ui.add(
            egui::TextEdit::multiline(&mut self.scanned_text)
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
    }

    fn generate_qr(&mut self, ctx: &egui::Context) {
        let text = self.qr_input.trim();
        if text.is_empty() {
            self.generator_status = "متن خالی است.".to_owned();
            return;
        }

        match QrCode::new(text.as_bytes()) {
            Ok(code) => {
                let img: GrayImage = code
                    .render::<Luma<u8>>()
                    .quiet_zone(true)
                    .min_dimensions(512, 512)
                    .build();

                let rgba = gray_to_rgba(&img);
                let size = [img.width() as usize, img.height() as usize];
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);

                self.qr_texture = Some(ctx.load_texture(
                    "generated_qr",
                    color_image,
                    egui::TextureOptions::NEAREST,
                ));
                self.qr_image = Some(img);
                self.generator_status = "QR ساخته شد.".to_owned();
            }
            Err(err) => {
                self.generator_status = format!("خطا در ساخت QR: {err}");
            }
        }
    }

    fn save_qr_png(&mut self) {
        let Some(img) = &self.qr_image else {
            self.generator_status = "اول QR را بساز.".to_owned();
            return;
        };

        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("qr_output.png");

        match img.save(&path) {
            Ok(()) => {
                self.generator_status = format!("ذخیره شد: {}", path.display());
            }
            Err(err) => {
                self.generator_status = format!("خطا در ذخیره PNG: {err}");
            }
        }
    }

    fn start_scanner(&mut self) {
        if self.scanner.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);

        let join = thread::spawn(move || scanner_worker(worker_stop, tx));

        self.scanner = Some(ScannerHandle {
            stop,
            rx,
            join: Some(join),
        });
        self.scanner_status = "در حال روشن کردن دوربین...".to_owned();
    }

    fn stop_scanner(&mut self) {
        if let Some(mut scanner) = self.scanner.take() {
            scanner.stop.store(true, Ordering::Relaxed);
            if let Some(join) = scanner.join.take() {
                let _ = join.join();
            }
            self.scanner_status = "اسکنر خاموش شد.".to_owned();
        }
    }

    fn poll_scanner_messages(&mut self) {
        if let Some(scanner) = &self.scanner {
            while let Ok(message) = scanner.rx.try_recv() {
                match message {
                    ScanMessage::Found(text) => {
                        self.scanned_text = text;
                        self.scanner_status = "QR خوانده شد.".to_owned();
                    }
                    ScanMessage::Status(status) => {
                        self.scanner_status = status;
                    }
                    ScanMessage::Error(err) => {
                        self.scanner_status = format!("خطا: {err}");
                    }
                }
            }
        }
    }
}

fn scanner_worker(stop: Arc<AtomicBool>, tx: mpsc::Sender<ScanMessage>) {
    let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = match Camera::new(CameraIndex::Index(0), requested) {
        Ok(camera) => camera,
        Err(err) => {
            let _ = tx.send(ScanMessage::Error(format!(
                "دوربین پیدا نشد یا دسترسی داده نشده: {err}"
            )));
            return;
        }
    };

    if let Err(err) = camera.open_stream() {
        let _ = tx.send(ScanMessage::Error(format!(
            "باز کردن استریم دوربین ناموفق بود: {err}"
        )));
        return;
    }

    let _ = tx.send(ScanMessage::Status(
        "دوربین روشن است؛ QR را جلوی دوربین بگیر.".to_owned(),
    ));

    let mut last_result = String::new();

    while !stop.load(Ordering::Relaxed) {
        match camera.frame() {
            Ok(frame) => match frame.decode_image::<RgbFormat>() {
                Ok(rgb_image) => {
                    if let Some(text) = decode_qr_from_rgb_image(&rgb_image) {
                        if text != last_result {
                            last_result = text.clone();
                            let _ = tx.send(ScanMessage::Found(text));
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(ScanMessage::Status(format!(
                        "فریم گرفته شد ولی decode نشد: {err}"
                    )));
                }
            },
            Err(err) => {
                let _ = tx.send(ScanMessage::Status(format!(
                    "در انتظار فریم دوربین: {err}"
                )));
            }
        }

        thread::sleep(Duration::from_millis(120));
    }

    let _ = camera.stop_stream();
}

fn decode_qr_from_rgb_image(img: &image::RgbImage) -> Option<String> {
    let (width, height) = img.dimensions();
    let raw = img.as_raw();

    let gray = GrayImage::from_fn(width, height, |x, y| {
        let i = ((y * width + x) * 3) as usize;
        let r = raw[i] as u32;
        let g = raw[i + 1] as u32;
        let b = raw[i + 2] as u32;
        let luma = ((299 * r + 587 * g + 114 * b) / 1000) as u8;
        Luma([luma])
    });

    let mut prepared = rqrr::PreparedImage::prepare(gray);
    for grid in prepared.detect_grids() {
        if let Ok((_meta, content)) = grid.decode() {
            return Some(content);
        }
    }

    None
}

fn gray_to_rgba(img: &GrayImage) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((img.width() * img.height() * 4) as usize);
    for pixel in img.pixels() {
        let value = pixel.0[0];
        rgba.extend_from_slice(&[value, value, value, 255]);
    }
    rgba
}

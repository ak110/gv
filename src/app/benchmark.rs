//! 表示時間の測定 (開発者が`mise run bench`で明示実行する)
//!
//! 隠しウィンドウの`AppWindow`で、実際のファイル読込・デコード・先読み・Direct2D描画を通して
//! 初回表示、連続した前方移動、直後の逆方向移動の時間を測る。
//! 開始は表示を要求する直前、終了は対象画像の描画が成功（`EndDraw`成功）した時点とする。
//! 結果は`target/gv-bench/`へJSONで保存し、`scripts/compare_display_bench.py`で2結果を比較する。
//! 通常の`cargo test`とCIでは実行しない (`#[ignore]`)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

/// 1回の操作の測定結果
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Trial {
    pub(crate) input: String,
    pub(crate) scenario: String,
    pub(crate) round: u32,
    pub(crate) step: u32,
    /// 表示対象のファイルリスト上の位置
    pub(crate) index: usize,
    pub(crate) ms: f64,
    /// 期限内に対象画像の描画が成功したか (失敗した試行は集計から除く)
    pub(crate) drawn: bool,
    /// 要求の時点で対象が先読み済みだったか
    pub(crate) cached: bool,
}

/// 入力種別×シナリオごとの集計
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ScenarioSummary {
    pub(crate) input: String,
    pub(crate) scenario: String,
    /// 集計に含めた (描画が成功した) 試行数
    pub(crate) count: usize,
    /// 集計から除いた (失敗・描画未完了の) 試行数
    pub(crate) failed: usize,
    /// 集計に含めた試行のうち先読み済みだった数
    pub(crate) cached: usize,
    pub(crate) median_ms: Option<f64>,
    pub(crate) p95_ms: Option<f64>,
}

/// 試行を入力種別×シナリオで集計する。描画が成功しなかった試行は値から除き、件数だけ数える
pub(crate) fn summarize(trials: &[Trial]) -> Vec<ScenarioSummary> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: BTreeMap<(String, String), Vec<&Trial>> = BTreeMap::new();
    for trial in trials {
        let key = (trial.input.clone(), trial.scenario.clone());
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(trial);
    }
    order
        .into_iter()
        .map(|key| {
            let group = &groups[&key];
            let mut values: Vec<f64> = group.iter().filter(|t| t.drawn).map(|t| t.ms).collect();
            values.sort_by(f64::total_cmp);
            ScenarioSummary {
                count: values.len(),
                failed: group.iter().filter(|t| !t.drawn).count(),
                cached: group.iter().filter(|t| t.drawn && t.cached).count(),
                median_ms: median(&values),
                p95_ms: percentile_nearest_rank(&values, 95),
                input: key.0,
                scenario: key.1,
            }
        })
        .collect()
}

/// 昇順に並んだ値の中央値
fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some(f64::midpoint(sorted[n / 2 - 1], sorted[n / 2])),
    }
}

/// 昇順に並んだ値の最近順位法によるパーセンタイル
fn percentile_nearest_rank(sorted: &[f64], percent: usize) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percent * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

/// 測定条件
#[derive(Debug, Clone, Serialize)]
struct Settings {
    /// 入力ごとに新しいウィンドウで繰り返す回数
    rounds: u32,
    /// 初回表示の後に続ける前方移動の回数
    forward_steps: u32,
    /// 前方移動の直後に続ける逆方向移動の回数
    backward_steps: u32,
    /// 操作の間隔 (前の画像の描画成功から次の要求まで。間にメッセージを処理する)
    interval_ms: u64,
    /// 1操作の描画完了を待つ上限
    timeout_ms: u64,
}

/// 素材の内容
#[derive(Debug, Clone, Serialize)]
struct Material {
    width: u32,
    height: u32,
    /// 入力ごとの画像枚数 (PDFはページ数)
    count: usize,
    /// ファイルリスト上の並び (名前の自然順で、生成した順と同じ)
    order: String,
    inputs: Vec<String>,
}

/// 実行環境
#[derive(Debug, Clone, Serialize)]
struct Environment {
    commit: String,
    /// 未commitの変更があったか
    dirty: bool,
    profile: &'static str,
    rustc: String,
    os: String,
    cpu: String,
    gpu: String,
}

/// 先読みの設定
#[derive(Debug, Clone, Serialize)]
struct PrefetchSettings {
    cache_budget_bytes: usize,
    base_image_size_bytes: usize,
}

/// 保存する測定結果
#[derive(Debug, Clone, Serialize)]
struct Report {
    environment: Environment,
    material: Material,
    settings: Settings,
    prefetch: PrefetchSettings,
    window_client_size: (u32, u32),
    /// 各値の意味 (結果ファイル単独で読めるようにする)
    notes: Vec<&'static str>,
    trials: Vec<Trial>,
    summary: Vec<ScenarioSummary>,
}

/// 固定内容の写真風画像 (滑らかなグラデーションと細かい模様で圧縮率を写真に近づける)
fn material_image(width: u32, height: u32, seed: u32) -> image::RgbImage {
    image::RgbImage::from_fn(width, height, |x, y| {
        let noise = (x.wrapping_mul(7919) ^ y.wrapping_mul(104_729) ^ seed.wrapping_mul(31)) % 23;
        image::Rgb([
            ((x * 255 / width + seed * 40 + noise) % 256) as u8,
            ((y * 255 / height + noise * 3) % 256) as u8,
            (((x + y) / 4 + seed * 17) % 256) as u8,
        ])
    })
}

fn encode(img: &image::RgbImage, format: image::ImageFormat) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, format).expect("encode material");
    buf.into_inner()
}

/// JPEGを1枚ずつページへ貼ったPDFを組み立てる
fn build_pdf(jpegs: &[Vec<u8>], width: u32, height: u32) -> Vec<u8> {
    // Windowsの既定描画 (96DPI) で画像と同じ画素数になるページ寸法
    let page_w = f64::from(width) * 72.0 / 96.0;
    let page_h = f64::from(height) * 72.0 / 96.0;
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let mut add_object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let id = offsets.len();
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    let page_ids: Vec<usize> = (0..jpegs.len()).map(|i| 3 + i * 3).collect();
    add_object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
    add_object(
        &mut out,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            jpegs.len()
        )
        .as_bytes(),
    );
    for (i, jpeg) in jpegs.iter().enumerate() {
        let page_id = page_ids[i];
        let content_id = page_id + 1;
        let image_id = page_id + 2;
        add_object(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w} {page_h}] \
                 /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>"
            )
            .as_bytes(),
        );
        let content = format!("q {page_w} 0 0 {page_h} 0 0 cm /Im0 Do Q");
        add_object(
            &mut out,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
            .as_bytes(),
        );
        let mut image_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        image_obj.extend_from_slice(jpeg);
        image_obj.extend_from_slice(b"\nendstream");
        add_object(&mut out, &image_obj);
    }
    let xref_offset = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

/// 素材を生成し、入力種別と開くパスの組を返す
fn prepare_inputs(dir: &Path, width: u32, height: u32, count: usize) -> Vec<(String, PathBuf)> {
    use std::io::Write as _;
    let jpeg_dir = dir.join("jpeg");
    let png_dir = dir.join("png");
    std::fs::create_dir_all(&jpeg_dir).unwrap();
    std::fs::create_dir_all(&png_dir).unwrap();
    let zip_path = dir.join("images.zip");
    let pdf_path = dir.join("pages.pdf");
    let mut zip = ::zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    let zip_options = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Stored);
    let mut jpegs = Vec::new();
    for i in 0..count {
        let img = material_image(width, height, i as u32);
        let name = format!("image_{i:03}");
        let jpeg = encode(&img, image::ImageFormat::Jpeg);
        std::fs::write(jpeg_dir.join(format!("{name}.jpg")), &jpeg).unwrap();
        std::fs::write(
            png_dir.join(format!("{name}.png")),
            encode(&img, image::ImageFormat::Png),
        )
        .unwrap();
        zip.start_file(format!("{name}.jpg"), zip_options).unwrap();
        zip.write_all(&jpeg).unwrap();
        jpegs.push(jpeg);
    }
    zip.finish().unwrap();
    std::fs::write(&pdf_path, build_pdf(&jpegs, width, height)).unwrap();
    vec![
        ("jpeg-folder".to_string(), jpeg_dir.join("image_000.jpg")),
        ("png-folder".to_string(), png_dir.join("image_000.png")),
        ("zip".to_string(), zip_path),
        ("pdf".to_string(), pdf_path),
    ]
}

/// 待機中のウィンドウメッセージ (先読み完了の通知など) を処理する
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
    };
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
}

/// 対象位置の画像の描画が成功するまで待ち、成功したかを返す
fn wait_until_drawn(app: &mut super::AppWindow, index: usize, deadline: Instant) -> bool {
    loop {
        pump_messages();
        app.process_document_events();
        if app.document.file_list().current_index() == Some(index)
            && app.document.current_image().is_some()
            && matches!(
                app.paint(),
                Ok(crate::render::d2d_renderer::DrawOutcome::Drawn)
            )
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 操作間隔の間もメッセージを処理して先読みを進める
fn idle(app: &mut super::AppWindow, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        pump_messages();
        app.process_document_events();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn powershell_value(expression: &str) -> String {
    command_output(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", expression],
    )
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "unknown".to_string())
}

fn environment() -> Environment {
    let repo = env!("CARGO_MANIFEST_DIR");
    Environment {
        commit: command_output("git", &["-C", repo, "rev-parse", "HEAD"])
            .unwrap_or_else(|| "unknown".to_string()),
        dirty: command_output("git", &["-C", repo, "status", "--porcelain"])
            .is_none_or(|s| !s.is_empty()),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        rustc: command_output("rustc", &["-V"]).unwrap_or_else(|| "unknown".to_string()),
        os: powershell_value(
            "$o = Get-CimInstance Win32_OperatingSystem; \"$($o.Caption) $($o.Version)\"",
        ),
        cpu: powershell_value("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name"),
        gpu: powershell_value("(Get-CimInstance Win32_VideoController).Name -join ', '"),
    }
}

/// 1入力分の測定 (新しいウィンドウで初回表示・前方移動・逆方向移動を順に行う)
fn measure_input(input: &str, path: &Path, round: u32, settings: &Settings) -> Vec<Trial> {
    use crate::ui::key_config::Action;
    let mut app = super::test_support::TestApp::new();
    let timeout = Duration::from_millis(settings.timeout_ms);
    let interval = Duration::from_millis(settings.interval_ms);
    let mut trials = Vec::new();
    let mut record = |scenario: &str, step: u32, index: usize, start: Instant, drawn, cached| {
        trials.push(Trial {
            input: input.to_string(),
            scenario: scenario.to_string(),
            round,
            step,
            index,
            ms: start.elapsed().as_secs_f64() * 1000.0,
            drawn,
            cached,
        });
    };

    // 初回表示 (ファイル指定起動・ドロップと同じ`Document::open`)
    let start = Instant::now();
    let opened = app.document.open(path).is_ok();
    let drawn = opened && wait_until_drawn(&mut app, 0, start + timeout);
    record("initial", 0, 0, start, drawn, false);
    // 初回表示に失敗した入力は移動を測らない。失敗時もウィンドウの閉じ方は成功時と同じにする
    if drawn {
        let mut index = 0usize;
        for (scenario, action, steps) in [
            ("forward", Action::NavigateForward, settings.forward_steps),
            ("backward", Action::NavigateBack, settings.backward_steps),
        ] {
            for step in 1..=steps {
                idle(&mut app, interval);
                let target = if matches!(action, Action::NavigateForward) {
                    index + 1
                } else {
                    index - 1
                };
                let cached = app.document.is_cached(target);
                let start = Instant::now();
                app.execute_action(action);
                let drawn = wait_until_drawn(&mut app, target, start + timeout);
                record(scenario, step, target, start, drawn, cached);
                index = app.document.file_list().current_index().unwrap_or(target);
            }
        }
    }
    app.close_without_release();
    trials
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial(input: &str, scenario: &str, ms: f64, drawn: bool) -> Trial {
        Trial {
            input: input.to_string(),
            scenario: scenario.to_string(),
            round: 0,
            step: 0,
            index: 0,
            ms,
            drawn,
            cached: false,
        }
    }

    /// 失敗・描画未完了の試行は中央値とp95から除き、件数を別に数える
    #[test]
    fn summary_excludes_failed_trials() {
        let trials = vec![
            trial("zip", "forward", 10.0, true),
            trial("zip", "forward", 5000.0, false),
            trial("zip", "forward", 30.0, true),
            trial("zip", "forward", 20.0, true),
            trial("zip", "initial", 100.0, false),
            trial("pdf", "forward", 40.0, true),
            trial("pdf", "forward", 60.0, true),
        ];
        let summary = summarize(&trials);
        let get = |input: &str, scenario: &str| {
            summary
                .iter()
                .find(|s| s.input == input && s.scenario == scenario)
                .unwrap()
                .clone()
        };
        let zip_forward = get("zip", "forward");
        assert_eq!((zip_forward.count, zip_forward.failed), (3, 1));
        assert_eq!(zip_forward.median_ms, Some(20.0));
        assert_eq!(zip_forward.p95_ms, Some(30.0));
        let zip_initial = get("zip", "initial");
        assert_eq!((zip_initial.count, zip_initial.failed), (0, 1));
        assert_eq!(zip_initial.median_ms, None);
        assert_eq!(get("pdf", "forward").median_ms, Some(50.0));
        // 最初に現れた順を保つ
        assert_eq!(summary[0].input, "zip");
        assert_eq!(summary[0].scenario, "forward");
    }

    /// 初回表示に失敗した入力は失敗試行1件を返し、移動を測らずに戻る
    #[test]
    fn failed_initial_display_is_recorded() {
        let dir = super::super::test_support::unique_temp_dir("bench-fail");
        let settings = Settings {
            rounds: 1,
            forward_steps: 3,
            backward_steps: 2,
            interval_ms: 0,
            timeout_ms: 200,
        };
        let trials = measure_input("missing", &dir.join("missing.png"), 0, &settings);
        assert_eq!(trials.len(), 1);
        assert_eq!(trials[0].scenario, "initial");
        assert!(!trials[0].drawn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 表示時間の測定 (`mise run bench`で実行する)
    #[test]
    #[ignore = "Windows実機のGPUとウィンドウを使う実測のため、mise run benchで明示実行する"]
    fn display_benchmark() {
        let (width, height, count) = (1920u32, 1080u32, 16usize);
        let settings = Settings {
            rounds: 3,
            forward_steps: 10,
            backward_steps: 5,
            interval_ms: 150,
            timeout_ms: 10_000,
        };
        let work = std::env::temp_dir().join(format!("gv_bench_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let inputs = prepare_inputs(&work, width, height, count);

        let mut trials = Vec::new();
        for round in 0..settings.rounds {
            for (input, path) in &inputs {
                trials.extend(measure_input(input, path, round, &settings));
            }
        }

        let probe = crate::app::test_support::TestApp::new();
        let window_client_size = crate::ui::window::get_client_size(probe.hwnd);
        probe.destroy();
        let report = Report {
            environment: environment(),
            material: Material {
                width,
                height,
                count,
                order: "image_000からの名前順 (PDFはページ順)".to_string(),
                inputs: inputs.iter().map(|(name, _)| name.clone()).collect(),
            },
            prefetch: PrefetchSettings {
                cache_budget_bytes: super::super::AppWindow::get_cache_budget(),
                base_image_size_bytes: crate::config::Config::default().prefetch.base_image_size(),
            },
            settings,
            window_client_size,
            notes: vec![
                "msは要求の直前から対象画像の描画成功 (EndDraw成功) までの経過時間",
                "drawn=falseの試行は期限内に描画が完了しなかったもので、summaryの値から除き件数だけをfailedへ数える",
                "cachedは要求の時点で対象が先読み済みだったか",
                "素材は測定ごとに一時フォルダへ再生成する。OSのファイルキャッシュは排除していない",
            ],
            summary: summarize(&trials),
            trials,
        };
        let _ = std::fs::remove_dir_all(&work);

        let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/gv-bench");
        std::fs::create_dir_all(&out_dir).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let out = out_dir.join(format!("display-{stamp}.json"));
        std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        for s in &report.summary {
            println!(
                "{:<12} {:<9} n={:<3} failed={:<2} cached={:<3} median={:>8.2}ms p95={:>8.2}ms",
                s.input,
                s.scenario,
                s.count,
                s.failed,
                s.cached,
                s.median_ms.unwrap_or(f64::NAN),
                s.p95_ms.unwrap_or(f64::NAN),
            );
        }
        println!("結果: {}", out.display());
    }
}

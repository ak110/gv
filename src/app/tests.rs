use super::export::{ExportFormat, write_image_to_path};
use super::test_support::TestApp;
use super::*;
use crate::render::layout::DisplayMode;
use crate::test_helpers::{TempDir, solid_image, write_png};
use std::fs;

#[test]
fn synchronous_paint_and_size_are_deferred_until_borrow_ends() {
    use windows::Win32::Foundation::RECT;

    let app = TestApp::new();
    app.with_app(|state| {
        state
            .document
            .apply_edit(solid_image(20, 20, [10, 20, 30, 255]));
    });
    app.with_app(super::AppWindow::process_document_events);
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    app.with_app(super::AppWindow::on_paint);
    let hwnd = app.hwnd();
    let listview = app.borrow().file_list_panel.listview_hwnd();
    let before = app.borrow().redraw_request_count.get();
    let mut rect = RECT::default();
    let mut panel_dimensions = || {
        // SAFETY: listviewはTestAppの破棄まで生存し、rectは書き込み可能。
        unsafe { GetWindowRect(listview, &raw mut rect).unwrap() };
        (rect.right - rect.left, rect.bottom - rect.top)
    };
    app.with_app(|state| {
        let before_dimensions = panel_dimensions();
        unsafe {
            SendMessageW(
                hwnd,
                WM_SIZE,
                Some(WPARAM(0)),
                Some(LPARAM((200 << 16) | 0x012c)),
            );
            SendMessageW(
                hwnd,
                WM_SIZE,
                Some(WPARAM(0)),
                Some(LPARAM((240 << 16) | 0x0168)),
            );
        }
        assert_eq!(state.redraw_request_count.get(), before);
        assert_eq!(panel_dimensions(), before_dimensions);
    });
    assert!(app.borrow().redraw_request_count.get() > before);
    assert_eq!(panel_dimensions().1, 240);

    let before_paint = app.borrow().redraw_request_count.get();
    app.with_app(|state| {
        state.renderer.simulate_target_loss_on_next_draw();
        unsafe { SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0))) };
        assert_eq!(state.redraw_request_count.get(), before_paint);
        assert!(state.renderer.target_snapshot().is_some());
    });
    assert!(app.borrow().redraw_request_count.get() > before_paint);
    unsafe { SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0))) };
    assert!(app.borrow().renderer.target_snapshot().is_none());
    unsafe { SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0))) };
    assert!(app.borrow().renderer.target_snapshot().is_some());
    assert!(app.borrow().renderer.last_draw_rect().is_some());
    app.destroy();
}

#[test]
fn modal_reentry_paints_resizes_and_skips_result_after_parent_destroy() {
    use std::cell::Cell;
    use std::rc::Rc;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;

    let dir = TempDir::new("modal_reentry");
    let app = Rc::new(TestApp::new());
    app.with_app(|state| {
        state.pasted_images = file_ops::PastedImageFiles::new(dir.path().to_path_buf());
        state.open_pasted_image(&solid_image(20, 20, [10, 20, 30, 255]));
    });
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    let hwnd = app.hwnd();
    let listview = app.borrow().file_list_panel.listview_hwnd();
    let run_app = Rc::clone(&app);
    let finished = Rc::new(Cell::new(false));
    let finish_flag = Rc::clone(&finished);
    app.with_app(|state| {
        state.defer_modal(
            move || {
                assert!(run_app.borrow().document.current_image().is_some());
                unsafe {
                    let _ = EnableWindow(hwnd, false);
                    SendMessageW(
                        hwnd,
                        WM_SIZE,
                        Some(WPARAM(0)),
                        Some(LPARAM((190 << 16) | 0x0140)),
                    );
                }
                let mut rect = RECT::default();
                unsafe { GetWindowRect(listview, &raw mut rect).unwrap() };
                assert_eq!(rect.bottom - rect.top, 190);
                run_app.with_app(|state| state.renderer.simulate_target_loss_on_next_draw());
                unsafe { SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0))) };
                assert!(run_app.borrow().renderer.target_snapshot().is_none());
                unsafe {
                    SendMessageW(hwnd, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)));
                    let _ = EnableWindow(hwnd, true);
                }
                assert!(run_app.borrow().renderer.target_snapshot().is_some());
                assert!(run_app.borrow().renderer.last_draw_rect().is_some());
                42
            },
            move |_, result| {
                assert_eq!(result, 42);
                finish_flag.set(true);
            },
        );
    });
    assert!(finished.get());

    let pasted = app.borrow().document.current_path().unwrap().to_path_buf();
    assert!(pasted.exists());
    finished.set(false);
    let run_app = Rc::clone(&app);
    let finish_flag = Rc::clone(&finished);
    app.with_app(|state| {
        state.defer_modal(
            move || {
                assert!(run_app.borrow().document.current_image().is_some());
                unsafe {
                    DestroyWindow(hwnd).unwrap();
                    assert!(!IsWindow(Some(hwnd)).as_bool());
                }
            },
            move |_, ()| finish_flag.set(true),
        );
    });
    assert!(!finished.get());
    assert!(!pasted.exists());
}

#[test]
fn modal_listview_text_notification_returns_label_within_buffer_limit() {
    use windows::Win32::UI::Controls::{
        LVIF_TEXT, LVITEMW, LVN_GETDISPINFOW, NMHDR, NMLVDISPINFOW,
    };
    use windows::core::PWSTR;

    let dir = TempDir::new("modal_notify");
    let path = dir.join("あいうえお.png");
    write_png(&path, &solid_image(4, 3, [10, 20, 30, 255]));
    let app = TestApp::new();
    app.open_image_file(&path);
    app.with_app(|state| state.execute_action(Action::MarkSet));
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    let hwnd = app.hwnd();
    let listview = app.borrow().file_list_panel.listview_hwnd();
    app.with_app(|state| {
        state.defer_modal(
            move || {
                let expected: Vec<u16> = "★● あいうえお.png".encode_utf16().collect();
                for capacity in [0usize, 1, 6, 64] {
                    let mut buffer = [0xFFFFu16; 64];
                    let mut info = NMLVDISPINFOW {
                        hdr: NMHDR {
                            hwndFrom: listview,
                            idFrom: usize::from(crate::ui::file_list_panel::FILE_LIST_CONTROL_ID),
                            code: LVN_GETDISPINFOW,
                        },
                        item: LVITEMW {
                            mask: LVIF_TEXT,
                            iItem: 0,
                            pszText: PWSTR(buffer.as_mut_ptr()),
                            cchTextMax: i32::try_from(capacity).unwrap(),
                            ..Default::default()
                        },
                    };
                    // SAFETY: WM_NOTIFYのNMLVDISPINFOWと書込先は同期SendMessageWが
                    // 戻るまでこのスコープに生存する。cchTextMaxはbufferの容量以下。
                    // 書込先と通知構造体は別のローカル変数で重ならない。
                    unsafe {
                        SendMessageW(
                            hwnd,
                            WM_NOTIFY,
                            Some(WPARAM(info.hdr.idFrom)),
                            Some(LPARAM(std::ptr::from_mut(&mut info) as isize)),
                        );
                    }
                    if capacity == 0 {
                        assert_eq!(buffer, [0xFFFF; 64]);
                    } else {
                        let written = expected.len().min(capacity - 1);
                        assert_eq!(&buffer[..written], &expected[..written]);
                        assert_eq!(buffer[written], 0);
                        assert!(buffer[written + 1..].iter().all(|&unit| unit == 0xFFFF));
                    }
                }
            },
            |_, ()| {},
        );
    });
    app.destroy();
}

#[test]
fn fullscreen_hides_panel_and_restores_image_origin() {
    let app = TestApp::new();
    app.with_app(|state| {
        state
            .document
            .apply_edit(solid_image(4096, 1, [30, 60, 90, 255]));
    });
    app.with_app(super::AppWindow::process_document_events);
    app.with_app(|state| state.execute_action(Action::DisplayAutoFit));
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    let panel_width = app.borrow().file_list_panel.panel_width();
    assert!(panel_width > 0);
    app.with_app(super::AppWindow::on_paint);
    assert!((app.borrow().renderer.last_draw_rect().unwrap().x - panel_width as f32).abs() < 0.01);

    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    assert!(app.borrow().file_list_panel.requested_visible());
    assert!(!app.borrow().file_list_panel.is_visible());
    assert_eq!(app.borrow().file_list_panel.panel_width(), 0);
    app.with_app(super::AppWindow::on_paint);
    assert!(app.borrow().renderer.last_draw_rect().unwrap().x.abs() < 0.01);

    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    assert!(app.borrow().file_list_panel.is_visible());
    assert_eq!(app.borrow().file_list_panel.panel_width(), panel_width);
    app.with_app(super::AppWindow::on_paint);
    assert!((app.borrow().renderer.last_draw_rect().unwrap().x - panel_width as f32).abs() < 0.01);

    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    assert!(!app.borrow().file_list_panel.requested_visible());
    assert!(!app.borrow().file_list_panel.is_visible());
    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    assert!(!app.borrow().file_list_panel.is_visible());
    app.with_app(super::AppWindow::on_paint);
    assert!(app.borrow().renderer.last_draw_rect().unwrap().x.abs() < 0.01);

    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    assert!(app.borrow().file_list_panel.requested_visible());
    assert!(!app.borrow().file_list_panel.is_visible());
    app.with_app(|state| state.execute_action(Action::ToggleFullscreen));
    assert!(app.borrow().file_list_panel.is_visible());
    app.with_app(super::AppWindow::on_paint);
    assert!((app.borrow().renderer.last_draw_rect().unwrap().x - panel_width as f32).abs() < 0.01);
    app.destroy();
}

#[derive(Debug, PartialEq)]
struct ViewState {
    image: Vec<u8>,
    size: (u32, u32),
    edited: bool,
    selection: Option<(i32, i32, i32, i32)>,
    mode: DisplayMode,
    draw_rect: Option<(f32, f32, f32, f32)>,
    panel_visible: bool,
}

fn view_state(state: &AppWindow) -> ViewState {
    let img = state.document.current_image().expect("image");
    ViewState {
        image: img.data.clone(),
        size: (img.width, img.height),
        edited: state.document.has_unsaved_edit(),
        selection: state
            .selection
            .current_rect()
            .map(|r| (r.x, r.y, r.width, r.height)),
        mode: state.renderer.layout().mode,
        draw_rect: state
            .renderer
            .last_draw_rect()
            .map(|r| (r.x, r.y, r.width, r.height)),
        panel_visible: state.file_list_panel.is_visible(),
    }
}

#[test]
fn paint_recovers_after_target_loss_keeping_state() {
    let dir = TempDir::new("paint_recover");
    let path = dir.join("image.png");
    write_png(&path, &solid_image(40, 30, [10, 200, 30, 128]));
    let app = TestApp::new();
    app.open_image_file(&path);
    app.with_app(|state| state.execute_action(Action::ToggleFileList));
    app.with_app(|state| state.execute_action(Action::ZoomIn));
    app.with_app(|state| state.execute_action(Action::FlipHorizontal));
    app.with_app(super::AppWindow::on_paint);
    app.drag_select((5, 5), (20, 15));
    app.with_app(super::AppWindow::on_paint);
    let before = view_state(&app.borrow());
    assert!(before.edited && before.panel_visible && before.selection.is_some());

    app.with_app(|state| state.renderer.simulate_target_loss_on_next_draw());
    assert!(app.paint_and_check_redraw(), "loss must request a redraw");
    assert!(app.borrow().renderer.target_snapshot().is_none());

    assert!(!app.paint_and_check_redraw());
    assert!(app.borrow().renderer.target_snapshot().is_some());
    assert_eq!(view_state(&app.borrow()), before);
    assert!(!app.title().contains("エラー"), "{}", app.title());

    app.destroy();
}

#[test]
fn paint_reports_recreate_failure_without_endless_redraw() {
    let dir = TempDir::new("paint_fail");
    let path = dir.join("image.png");
    write_png(&path, &solid_image(8, 8, [0, 0, 0, 255]));
    let app = TestApp::new();
    app.open_image_file(&path);
    app.with_app(super::AppWindow::on_paint);

    app.with_app(|state| state.renderer.simulate_target_loss_on_next_draw());
    assert!(app.paint_and_check_redraw());
    app.with_app(|state| state.renderer.fail_next_target_creation());
    assert!(!app.paint_and_check_redraw());
    assert!(
        app.title().contains("描画に失敗しました"),
        "{}",
        app.title()
    );

    let (w, h) = window::get_client_size(app.hwnd());
    let before = app.borrow().redraw_request_count.get();
    app.with_app(|state| state.on_size(w, h));
    assert_ne!(app.borrow().redraw_request_count.get(), before);
    assert!(!app.paint_and_check_redraw());
    assert!(app.borrow().renderer.target_snapshot().is_some());
    assert!(!app.title().contains("エラー"), "{}", app.title());

    for _ in 0..MAX_RENDER_RECOVERY_ATTEMPTS {
        app.with_app(|state| state.renderer.simulate_target_loss_on_next_draw());
        assert!(app.paint_and_check_redraw());
    }
    app.with_app(|state| state.renderer.simulate_target_loss_on_next_draw());
    assert!(!app.paint_and_check_redraw());
    assert!(
        app.title().contains("描画を復旧できませんでした"),
        "{}",
        app.title()
    );

    app.destroy();
}

#[test]
fn operation_failure_is_shown_and_survives_title_update() {
    let dir = TempDir::new("op_fail");
    let path = dir.join("image.png");
    write_png(&path, &solid_image(4, 4, [0, 0, 0, 255]));
    let app = TestApp::new();
    app.open_image_file(&path);

    let result: Result<Option<()>> = Err(anyhow::anyhow!("アクセスが拒否されました"));
    assert_eq!(
        app.with_app(|state| state.take_success("ファイルの移動", "移動先を確認する", result)),
        None
    );
    let title = app.title();
    assert!(
        title.contains(
            "ファイルの移動に失敗しました。移動先を確認する。詳細: アクセスが拒否されました"
        ),
        "{title}"
    );

    app.with_app(|state| state.document.reload());
    app.with_app(super::AppWindow::process_document_events);
    assert_eq!(app.title(), title);

    app.with_app(|state| {
        state.handle_paint_outcome(Err(anyhow::anyhow!("描画エラー")));
        state.handle_paint_outcome(Ok(super::DrawOutcome::Drawn));
    });
    assert_eq!(app.title(), title);

    app.with_app(super::AppWindow::begin_user_operation);
    assert!(!app.title().contains("エラー"), "{}", app.title());

    app.destroy();
}

#[test]
fn document_read_failure_is_shown() {
    let dir = TempDir::new("read_fail");
    let first = dir.join("a.png");
    let second = dir.join("b.png");
    write_png(&first, &solid_image(4, 4, [0, 0, 0, 255]));
    write_png(&second, &solid_image(4, 4, [255, 0, 0, 255]));
    let app = TestApp::new();
    app.with_app(|state| state.document.open(&first).unwrap());
    app.with_app(super::AppWindow::process_document_events);
    assert!(app.borrow().document.current_image().is_some());
    fs::remove_file(&second).unwrap();
    let expected_error = fs::read(&second).unwrap_err();
    assert_eq!(expected_error.kind(), std::io::ErrorKind::NotFound);

    app.with_app(|state| state.execute_action(Action::NavigateForward));
    let title = app.title();
    assert!(title.contains("エラー"), "{title}");
    assert!(title.contains("画像を読み込めませんでした"), "{title}");
    let (_, detail) = title.split_once("詳細: ").expect("失敗の詳細");
    assert!(detail.contains("b.png"), "{detail}");
    assert!(detail.contains("画像読み込み失敗"), "{detail}");
    assert!(app.borrow().document.current_image().is_none());

    app.destroy();
}

#[test]
fn cancel_and_success_are_not_reported_as_failure() {
    let app = TestApp::new();
    let normal = app.title();

    assert_eq!(
        app.with_app(|state| state.take_success::<PathBuf>(
            "保存ダイアログの表示",
            "再操作する",
            Ok(None)
        )),
        None
    );
    let aborted: Result<bool> = Ok(false);
    assert_eq!(
        app.with_app(|state| state.take_success(
            "ファイルの削除",
            "再操作する",
            aborted.map(|done| done.then_some(()))
        )),
        None
    );
    assert_eq!(app.title(), normal);

    let dest = PathBuf::from("dest");
    assert_eq!(
        app.with_app(|state| state.take_success(
            "保存ダイアログの表示",
            "再操作する",
            Ok(Some(dest.clone()))
        )),
        Some(dest)
    );
    let done: Result<bool> = Ok(true);
    assert_eq!(
        app.with_app(|state| state.take_success(
            "ファイルの削除",
            "再操作する",
            done.map(|done| done.then_some(()))
        )),
        Some(())
    );
    assert_eq!(app.title(), normal);

    app.destroy();
}

#[test]
fn oriented_image_is_exported_as_displayed() {
    use crate::test_helpers::{
        asymmetric_rgba, encode_with_exif, exif_with_orientation, expected_oriented,
    };
    let dir = TempDir::new("oriented_export");
    let src = asymmetric_rgba(4, 3);
    let original = encode_with_exif(
        &src,
        image::ImageFormat::Png,
        Some(exif_with_orientation(6)),
    );
    let path = dir.join("photo.png");
    fs::write(&path, &original).unwrap();
    let displayed = expected_oriented(&src, 6);

    let app = TestApp::new();
    app.open_image_file(&path);
    app.with_app(super::AppWindow::on_paint);

    for (format, name, lossless) in [
        (ExportFormat::Png, "out.png", true),
        (ExportFormat::Bmp, "out.bmp", true),
        (ExportFormat::Jpg, "out.jpg", false),
    ] {
        let out = dir.join(name);
        app.with_app(|state| state.write_current_image(format, &out).unwrap());
        let written = image::open(&out).unwrap().into_rgba8();
        assert_eq!(written.dimensions(), (3, 4), "{name}");
        if lossless {
            assert_eq!(written.as_raw(), displayed.as_raw(), "{name}");
        }
    }

    app.drag_select((0, 1), (2, 3));
    let sel = app.borrow().selection.current_rect().expect("selected");
    let expected_crop = image::imageops::crop_imm(
        &displayed,
        sel.x as u32,
        sel.y as u32,
        sel.width as u32,
        sel.height as u32,
    )
    .to_image();
    let out = dir.join("selection.png");
    app.with_app(|state| state.write_current_image(ExportFormat::Png, &out).unwrap());
    assert_eq!(image::open(&out).unwrap().into_rgba8(), expected_crop);
    app.with_app(|state| state.execute_action(Action::Crop));
    let state = app.borrow();
    let cropped = state.document.current_image().unwrap();
    assert_eq!((cropped.width, cropped.height), expected_crop.dimensions());
    assert_eq!(&cropped.data, expected_crop.as_raw());

    assert_eq!(fs::read(&path).unwrap(), original);
    drop(state);
    app.destroy();
}

#[test]
fn write_png_with_extensionless_path_succeeds() {
    let image = solid_image(1, 1, [255, 255, 255, 255]);
    let (w, h, rgba) = (image.width, image.height, image.data);
    let dir = TempDir::new("export");
    let path = dir.join("png_no_ext");
    let result = write_image_to_path(w, h, &rgba, ExportFormat::Png, &path);
    assert!(
        result.is_ok(),
        "extensionless path should succeed: {result:?}"
    );
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
}

#[test]
fn write_jpg_with_txt_extension_writes_jpeg_bytes() {
    let image = solid_image(8, 4, [40, 120, 200, 255]);
    let (w, h, rgba) = (image.width, image.height, image.data);
    let dir = TempDir::new("export");
    let path = dir.join("mismatch.txt");
    let result = write_image_to_path(w, h, &rgba, ExportFormat::Jpg, &path);
    assert!(
        result.is_ok(),
        "txt extension with Jpg format should succeed: {result:?}"
    );
    let bytes = fs::read(&path).unwrap();
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
        .unwrap()
        .into_rgb8();
    assert_eq!(decoded.dimensions(), (8, 4));
    for (&actual, expected) in decoded.get_pixel(4, 2).0.iter().zip([40u8, 120, 200]) {
        assert!(actual.abs_diff(expected) <= 4, "{actual} vs {expected}");
    }
}

#[test]
fn write_bmp_with_extensionless_path_succeeds() {
    let image = solid_image(3, 2, [40, 120, 200, 255]);
    let (w, h, rgba) = (image.width, image.height, image.data);
    let dir = TempDir::new("export");
    let path = dir.join("bmp_no_ext");
    let result = write_image_to_path(w, h, &rgba, ExportFormat::Bmp, &path);
    assert!(
        result.is_ok(),
        "bmp extensionless should succeed: {result:?}"
    );
    let bytes = fs::read(&path).unwrap();
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Bmp)
        .unwrap()
        .into_rgb8();
    assert_eq!(decoded.dimensions(), (3, 2));
    assert_eq!(*decoded.get_pixel(1, 1), image::Rgb([40, 120, 200]));
}

#[test]
fn export_format_returns_expected_extensions() {
    assert_eq!(ExportFormat::Png.extension(), "png");
    assert_eq!(ExportFormat::Jpg.extension(), "jpg");
    assert_eq!(ExportFormat::Bmp.extension(), "bmp");
}

#[test]
fn image_export_failure_keeps_existing_file_and_can_retry() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let dir = TempDir::new("export_preserve");
    let path = dir.join("保存画像.png");
    write_png(&path, &solid_image(4, 3, [10, 20, 30, 255]));
    let before = fs::read(&path).unwrap();
    let app = TestApp::new();
    app.open_image_file(&path);
    app.with_app(|state| state.execute_action(Action::InvertColors));
    let edited = app.borrow().document.current_image().unwrap().data.clone();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let result = app
        .with_app(|state| state.write_current_image(ExportFormat::Png, &path))
        .map(Some);
    assert!(
        app.with_app(|state| state.take_success("画像の出力", "保存先を確認する", result))
            .is_none()
    );
    assert!(app.title().contains("画像の出力に失敗しました"));
    drop(locked);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(app.borrow().document.current_image().unwrap().data, edited);
    assert!(app.borrow().document.has_unsaved_edit());

    app.with_app(|state| state.write_current_image(ExportFormat::Png, &path))
        .unwrap();
    assert_eq!(image::open(&path).unwrap().into_rgba8().into_raw(), edited);
    app.destroy();
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn partial_container_failure_shows_next_action() {
    let dir = TempDir::new("partial_container");
    let image = dir.join("image.png");
    write_png(&image, &solid_image(2, 2, [0, 0, 0, 255]));
    let broken = dir.join("broken.zip");
    fs::write(&broken, b"not a zip").unwrap();
    let app = TestApp::new();

    app.with_app(|state| state.document.open_multiple(&[image, broken]).unwrap());
    app.with_app(super::AppWindow::process_document_events);
    let title = app.title();
    assert!(
        title.contains(
            "1件のコンテナを開けませんでした。開けたコンテナの画像は表示を続けられます。"
        ),
        "{title}"
    );
    let (_, detail) = title.split_once("詳細: ").expect("失敗の詳細");
    assert!(detail.contains("broken.zip"), "{detail}");
    assert!(detail.contains("ZIPバッファの読み取りに失敗"), "{detail}");

    app.destroy();
}

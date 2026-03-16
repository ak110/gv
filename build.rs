fn main() {
    // アイコンリソースをコンパイル（.rcファイル経由で複数アイコンを登録）
    let mut res = winres::WindowsResource::new();
    res.set_resource_file("resources/gv3.rc");
    if let Err(e) = res.compile() {
        eprintln!("リソースコンパイル警告: {e}");
    }
}

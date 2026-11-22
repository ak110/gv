use windows::Win32::Graphics::Gdi::{
    CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_MODERN, FIXED_PITCH, HFONT,
    OUT_DEFAULT_PRECIS, PROOF_QUALITY,
};

pub struct MonospaceFont {
    hfont: HFONT,
}

impl MonospaceFont {
    pub fn new(size: i32) -> Self {
        let hfont = unsafe {
            CreateFontW(
                size,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                PROOF_QUALITY,
                (FF_MODERN.0 | FIXED_PITCH.0) as u32,
                windows::core::w!("Consolas"),
            )
        };
        Self { hfont }
    }

    pub fn hfont(&self) -> HFONT {
        self.hfont
    }
}

impl Drop for MonospaceFont {
    fn drop(&mut self) {
        if !self.hfont.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.hfont.into());
            }
        }
    }
}

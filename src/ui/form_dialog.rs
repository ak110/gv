use std::cell::Cell;
use std::rc::Rc;

use anyhow::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use super::dialog::{ID_EDIT_BASE, client_size, create_control, select_edit, show_dialog};
use crate::util::to_wide;

const ID_ERROR: i32 = 0x700;
const MARGIN: i32 = 12;
const FIELD_HEIGHT: i32 = 48;

#[derive(Clone, Copy, Debug)]
pub enum NumberRule {
    Integer { min: i64, max: i64 },
    Decimal { min: Option<f64>, max: Option<f64> },
}

impl NumberRule {
    pub fn accepts(self, text: &str) -> bool {
        match self {
            Self::Integer { min, max } => text
                .trim()
                .parse::<i64>()
                .is_ok_and(|value| (min..=max).contains(&value)),
            Self::Decimal { min, max } => text.trim().parse::<f64>().is_ok_and(|value| {
                value.is_finite()
                    && min.is_none_or(|min| value >= min)
                    && max.is_none_or(|max| value <= max)
            }),
        }
    }

    pub fn range_label(self) -> String {
        match self {
            Self::Integer { min, max } => format!("{min}〜{max}、整数"),
            Self::Decimal {
                min: Some(min),
                max: Some(max),
            } => format!("{min}〜{max}"),
            Self::Decimal {
                min: None,
                max: None,
            } => "有限の数値".into(),
            Self::Decimal {
                min: Some(min),
                max: None,
            } => format!("{min}以上"),
            Self::Decimal {
                min: None,
                max: Some(max),
            } => format!("{max}以下"),
        }
    }
}

#[derive(Clone)]
pub struct FieldDef {
    pub label: String,
    pub default: String,
    pub rule: NumberRule,
}

impl FieldDef {
    pub fn display_label(&self) -> String {
        format!("{} ({})", self.label, self.rule.range_label())
    }
}

pub fn show_form_dialog(
    parent: HWND,
    title: &str,
    fields: &[FieldDef],
) -> Result<Option<Vec<String>>> {
    if fields.is_empty() {
        return Ok(None);
    }
    let height = MARGIN * 4
        + fields.len() as i32 * FIELD_HEIGHT
        + 64
        + unsafe { GetSystemMetrics(SM_CYCAPTION) + GetSystemMetrics(SM_CYSIZEFRAME) };
    let values = Rc::new(Cell::new(None));
    show_dialog(
        parent,
        title,
        (360, height),
        |hwnd| create_form(hwnd, fields),
        {
            let values = Rc::clone(&values);
            let fields = fields.to_vec();
            move |hwnd| {
                let Some(input) = confirm_form(hwnd, &fields) else {
                    return false;
                };
                values.set(Some(input));
                true
            }
        },
    )?;
    Ok(values.take())
}

fn create_form(hwnd: HWND, fields: &[FieldDef]) -> Result<()> {
    let (width, _) = client_size(hwnd)?;
    for (i, field) in fields.iter().enumerate() {
        let y = MARGIN + i as i32 * FIELD_HEIGHT;
        let id = ID_EDIT_BASE + i as i32;
        create_control(
            hwnd,
            "STATIC",
            &field.display_label(),
            WS_CHILD | WS_VISIBLE,
            WINDOW_EX_STYLE::default(),
            0,
            (MARGIN, y, width - MARGIN * 2, 20),
        )?;
        create_control(
            hwnd,
            "EDIT",
            &field.default,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            if (ID_EDIT_BASE..ID_ERROR).contains(&id) {
                WS_EX_CLIENTEDGE
            } else {
                WINDOW_EX_STYLE::default()
            },
            id,
            (MARGIN, y + 20, width - MARGIN * 2, 24),
        )?;
    }
    let y = MARGIN + fields.len() as i32 * FIELD_HEIGHT;
    create_control(
        hwnd,
        "STATIC",
        "",
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        ID_ERROR,
        (MARGIN, y, width - MARGIN * 2, 40),
    )?;
    let x = (width - 168) / 2;
    create_control(
        hwnd,
        "BUTTON",
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        1,
        (x, y + 44, 80, 28),
    )?;
    create_control(
        hwnd,
        "BUTTON",
        "キャンセル",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        2,
        (x + 88, y + 44, 80, 28),
    )?;
    Ok(())
}

fn confirm_form(hwnd: HWND, fields: &[FieldDef]) -> Option<Vec<String>> {
    let values: Vec<String> = (0..fields.len())
        .map(|i| {
            let edit =
                unsafe { GetDlgItem(Some(hwnd), ID_EDIT_BASE + i as i32) }.unwrap_or_default();
            read_edit_text(edit)
        })
        .collect();
    if let Some((index, field)) = fields
        .iter()
        .enumerate()
        .find(|(i, field)| !field.rule.accepts(&values[*i]))
    {
        let message = to_wide(&format!(
            "{}に従って入力し直してください。",
            field.display_label()
        ));
        unsafe {
            if let Ok(error) = GetDlgItem(Some(hwnd), ID_ERROR) {
                let _ = SetWindowTextW(error, PCWSTR(message.as_ptr()));
            }
            if let Ok(edit) = GetDlgItem(Some(hwnd), ID_EDIT_BASE + index as i32) {
                select_edit(edit);
            }
        }
        return None;
    }
    Some(values)
}

fn read_edit_text(edit: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        let mut text = vec![0u16; len + 1];
        let copied = GetWindowTextW(edit, &mut text).max(0) as usize;
        String::from_utf16_lossy(&text[..copied]).trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_inputs_reject_overflow_non_numbers_and_non_finite_values() {
        for (rule, valid, invalid) in [
            (
                NumberRule::Integer { min: 1, max: 10 },
                vec!["1", "10", " 5 "],
                vec!["0", "11", "1.5", "abc", "", "9223372036854775808"],
            ),
            (
                NumberRule::Decimal {
                    min: Some(0.1),
                    max: Some(10.0),
                },
                vec!["0.1", "10", "1e0"],
                vec!["0", "10.1", "NaN", "inf", "-inf", "abc", ""],
            ),
            (
                NumberRule::Decimal {
                    min: None,
                    max: None,
                },
                vec!["-360", "0", "360.5"],
                vec!["NaN", "inf", "-inf", "1e999", "abc", ""],
            ),
        ] {
            for value in valid {
                assert!(rule.accepts(value), "{rule:?}: {value}");
            }
            for value in invalid {
                assert!(!rule.accepts(value), "{rule:?}: {value}");
            }
        }
    }
}

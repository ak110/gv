use std::io::Cursor;

use exif::{In, Tag};

pub fn read_exif_fields(data: &[u8]) -> Vec<(String, String)> {
    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut Cursor::new(data)) else {
        return Vec::new();
    };

    let mut fields = Vec::new();

    if let Some(v) = get_string(&exif, Tag::Model) {
        fields.push(("カメラ".to_string(), v));
    }
    if let Some(v) = get_string(&exif, Tag::LensModel) {
        fields.push(("レンズ".to_string(), v));
    }
    if let Some(v) = format_focal_length(&exif) {
        fields.push(("焦点距離".to_string(), v));
    }
    if let Some(v) = format_fnumber(&exif) {
        fields.push(("F値".to_string(), v));
    }
    if let Some(v) = format_exposure_time(&exif) {
        fields.push(("シャッター速度".to_string(), v));
    }
    if let Some(v) = get_uint(&exif, Tag::PhotographicSensitivity) {
        fields.push(("ISO感度".to_string(), format!("ISO {v}")));
    }
    if let Some(v) = get_string(&exif, Tag::DateTimeOriginal) {
        fields.push(("撮影日時".to_string(), v));
    }
    if let Some(v) = format_gps(&exif) {
        fields.push(("GPS".to_string(), v));
    }

    fields
}

fn get_string(exif: &exif::Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let s = field.display_value().to_string();
    let trimmed = s.trim().trim_matches('"').trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn get_uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    field.value.get_uint(0)
}

fn get_rational(exif: &exif::Exif, tag: Tag) -> Option<exif::Rational> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    if let exif::Value::Rational(ref v) = field.value {
        v.first().copied()
    } else {
        None
    }
}

fn rational_to_f64(r: exif::Rational) -> Option<f64> {
    (r.denom != 0).then(|| f64::from(r.num) / f64::from(r.denom))
}

fn format_focal_length(exif: &exif::Exif) -> Option<String> {
    let mm = rational_to_f64(get_rational(exif, Tag::FocalLength)?)?;
    if (mm - mm.round()).abs() < 0.01 {
        Some(format!("{} mm", mm.round() as u32))
    } else {
        Some(format!("{mm:.1} mm"))
    }
}

fn format_fnumber(exif: &exif::Exif) -> Option<String> {
    let f = rational_to_f64(get_rational(exif, Tag::FNumber)?)?;
    if (f - f.round()).abs() < 0.01 {
        Some(format!("f/{}", f.round() as u32))
    } else {
        Some(format!("f/{f:.1}"))
    }
}

fn format_exposure_time(exif: &exif::Exif) -> Option<String> {
    let r = get_rational(exif, Tag::ExposureTime)?;
    let secs = rational_to_f64(r)?;
    if r.num == 0 {
        return None;
    }
    if r.num >= r.denom {
        if (secs - secs.round()).abs() < 0.01 {
            Some(format!("{} 秒", secs.round() as u32))
        } else {
            Some(format!("{secs:.1} 秒"))
        }
    } else {
        Some(format!("{}/{} 秒", r.num, r.denom))
    }
}

fn format_gps(exif: &exif::Exif) -> Option<String> {
    let lat = gps_to_decimal(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef)?;
    let lon = gps_to_decimal(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef)?;
    Some(format!("{lat:.6}, {lon:.6}"))
}

fn gps_to_decimal(exif: &exif::Exif, coord_tag: Tag, ref_tag: Tag) -> Option<f64> {
    let field = exif.get_field(coord_tag, In::PRIMARY)?;
    let rationals = if let exif::Value::Rational(ref v) = field.value {
        if v.len() >= 3 {
            v
        } else {
            return None;
        }
    } else {
        return None;
    };

    let deg = rational_to_f64(rationals[0])?;
    let min = rational_to_f64(rationals[1])?;
    let sec = rational_to_f64(rationals[2])?;
    let mut decimal = deg + min / 60.0 + sec / 3600.0;

    if let Some(ref_field) = exif.get_field(ref_tag, In::PRIMARY) {
        let ref_str = ref_field.display_value().to_string();
        let ref_trimmed = ref_str.trim().trim_matches('"');
        if ref_trimmed == "S" || ref_trimmed == "W" {
            decimal = -decimal;
        }
    }

    Some(decimal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{
        ExifEntry, ExifIfd, ExifValue, asymmetric_rgba, create_1x1_white_png, encode_with_exif,
        exif_with_entries,
    };

    #[test]
    fn read_exif_fields_returns_empty_for_non_exif_data() {
        let png_data = create_1x1_white_png();
        let fields = read_exif_fields(&png_data);
        assert!(fields.is_empty());
    }

    #[test]
    fn read_exif_fields_returns_empty_for_invalid_data() {
        let fields = read_exif_fields(&[0, 1, 2, 3]);
        assert!(fields.is_empty());
    }

    #[test]
    fn read_exif_fields_returns_empty_for_empty_data() {
        let fields = read_exif_fields(&[]);
        assert!(fields.is_empty());
    }

    fn read_fields(entries: &[ExifEntry]) -> Vec<(String, String)> {
        let png = encode_with_exif(
            &asymmetric_rgba(2, 2),
            image::ImageFormat::Png,
            Some(exif_with_entries(entries)),
        );
        read_exif_fields(&png)
    }

    fn field(label: &str, value: &str) -> (String, String) {
        (label.to_string(), value.to_string())
    }

    fn rational(ifd: ExifIfd, tag: u16, values: &[(u32, u32)]) -> ExifEntry {
        ExifEntry::new(ifd, tag, ExifValue::Rational(values.to_vec()))
    }

    const MODEL: u16 = 0x0110;
    const EXPOSURE_TIME: u16 = 0x829A;
    const F_NUMBER: u16 = 0x829D;
    const ISO: u16 = 0x8827;
    const DATE_TIME_ORIGINAL: u16 = 0x9003;
    const FOCAL_LENGTH: u16 = 0x920A;
    const LENS_MODEL: u16 = 0xA434;
    const GPS_LATITUDE_REF: u16 = 0x0001;
    const GPS_LATITUDE: u16 = 0x0002;
    const GPS_LONGITUDE_REF: u16 = 0x0003;
    const GPS_LONGITUDE: u16 = 0x0004;

    fn gps_entries(lat_ref: &'static str, lon_ref: &'static str) -> Vec<ExifEntry> {
        vec![
            ExifEntry::new(ExifIfd::Gps, GPS_LATITUDE_REF, ExifValue::Ascii(lat_ref)),
            rational(ExifIfd::Gps, GPS_LATITUDE, &[(35, 1), (40, 1), (5245, 100)]),
            ExifEntry::new(ExifIfd::Gps, GPS_LONGITUDE_REF, ExifValue::Ascii(lon_ref)),
            rational(
                ExifIfd::Gps,
                GPS_LONGITUDE,
                &[(139, 1), (46, 1), (165, 100)],
            ),
        ]
    }

    #[test]
    fn read_exif_fields_lists_all_items_in_display_order() {
        let mut entries = vec![
            ExifEntry::new(ExifIfd::Primary, MODEL, ExifValue::Ascii("TestCam")),
            ExifEntry::new(ExifIfd::Exif, LENS_MODEL, ExifValue::Ascii("TestLens 50mm")),
            rational(ExifIfd::Exif, FOCAL_LENGTH, &[(50, 1)]),
            rational(ExifIfd::Exif, F_NUMBER, &[(28, 10)]),
            rational(ExifIfd::Exif, EXPOSURE_TIME, &[(1, 250)]),
            ExifEntry::new(ExifIfd::Exif, ISO, ExifValue::Long(6400)),
            ExifEntry::new(
                ExifIfd::Exif,
                DATE_TIME_ORIGINAL,
                ExifValue::Ascii("2026:10:25 06:00:00"),
            ),
        ];
        entries.extend(gps_entries("N", "E"));
        assert_eq!(
            read_fields(&entries),
            vec![
                field("カメラ", "TestCam"),
                field("レンズ", "TestLens 50mm"),
                field("焦点距離", "50 mm"),
                field("F値", "f/2.8"),
                field("シャッター速度", "1/250 秒"),
                field("ISO感度", "ISO 6400"),
                field("撮影日時", "2026-10-25 06:00:00"),
                field("GPS", "35.681236, 139.767125"),
            ]
        );
    }

    #[test]
    fn format_exposure_time_fraction() {
        let fields = read_fields(&[rational(ExifIfd::Exif, EXPOSURE_TIME, &[(1, 250)])]);
        assert_eq!(fields, vec![field("シャッター速度", "1/250 秒")]);
    }

    #[test]
    fn format_exposure_time_whole_seconds() {
        let fields = read_fields(&[rational(ExifIfd::Exif, EXPOSURE_TIME, &[(2, 1)])]);
        assert_eq!(fields, vec![field("シャッター速度", "2 秒")]);
    }

    #[test]
    fn gps_decimal_conversion() {
        assert_eq!(
            read_fields(&gps_entries("N", "E")),
            vec![field("GPS", "35.681236, 139.767125")]
        );
        assert_eq!(
            read_fields(&gps_entries("S", "W")),
            vec![field("GPS", "-35.681236, -139.767125")]
        );
    }

    #[test]
    fn focal_length_integer() {
        let fields = read_fields(&[rational(ExifIfd::Exif, FOCAL_LENGTH, &[(50, 1)])]);
        assert_eq!(fields, vec![field("焦点距離", "50 mm")]);
    }

    #[test]
    fn fnumber_decimal() {
        let fields = read_fields(&[rational(ExifIfd::Exif, F_NUMBER, &[(28, 10)])]);
        assert_eq!(fields, vec![field("F値", "f/2.8")]);
    }

    #[test]
    fn gps_with_zero_denominator_is_omitted_and_other_fields_remain() {
        let cases: [(u16, usize, (u32, u32)); 4] = [
            (GPS_LATITUDE, 0, (35, 0)),
            (GPS_LATITUDE, 1, (0, 0)),
            (GPS_LATITUDE, 2, (5245, 0)),
            (GPS_LONGITUDE, 0, (139, 0)),
        ];
        for (tag, position, broken) in cases {
            let mut entries = gps_entries("N", "E");
            for entry in &mut entries {
                if entry.tag == tag
                    && let ExifValue::Rational(values) = &mut entry.value
                {
                    values[position] = broken;
                }
            }
            entries.push(ExifEntry::new(
                ExifIfd::Primary,
                MODEL,
                ExifValue::Ascii("TestCam"),
            ));
            assert_eq!(
                read_fields(&entries),
                vec![field("カメラ", "TestCam")],
                "tag={tag:#06x} position={position}"
            );
        }
    }

    #[test]
    fn rational_fields_with_zero_denominator_are_omitted() {
        for tag in [FOCAL_LENGTH, F_NUMBER, EXPOSURE_TIME] {
            let fields = read_fields(&[
                ExifEntry::new(ExifIfd::Primary, MODEL, ExifValue::Ascii("TestCam")),
                rational(ExifIfd::Exif, tag, &[(28, 0)]),
            ]);
            assert_eq!(fields, vec![field("カメラ", "TestCam")], "tag={tag:#06x}");
        }
    }
}

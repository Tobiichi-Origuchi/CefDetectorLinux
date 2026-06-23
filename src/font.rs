pub fn load_cjk_font() -> Option<Vec<u8>> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    for family in [
        "Noto Sans CJK SC",
        "Noto Sans CJK",
        "WenQuanYi Micro Hei",
        "WenQuanYi Zen Hei",
        "Source Han Sans SC",
        "Noto Serif CJK SC",
    ] {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..Default::default()
        };
        if let Some(id) = db.query(&query) {
            let mut data = None;
            db.with_face_data(id, |d, _| data = Some(d.to_vec()));
            if data.is_some() {
                return data;
            }
        }
    }

    for face in db.faces() {
        for family in &face.families {
            let name = family.0.to_lowercase();
            if name.contains("cjk")
                || name.contains("wenquan")
                || name.contains("source han")
                || name.contains("han sans")
                || name.contains("han serif")
                || name.contains("songti")
                || name.contains("heiti")
            {
                let mut data = None;
                let id = face.id;
                db.with_face_data(id, |d, _| data = Some(d.to_vec()));
                if data.is_some() {
                    return data;
                }
            }
        }
    }

    None
}

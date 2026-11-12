//! Sweep folders: now [`jetcarp_api::sweeps`] (re-exported here for hosts that used this
//! path).

pub use jetcarp_api::sweeps::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn find_promote_favourite_and_sheet() {
        let root = std::env::temp_dir().join(format!("jetcarp-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = crate::run(
            r#"sweep(#{ rules: "W", from: 28, to: 31, tiling: "square", size: [40, 24],
                        boundary: "dead", seed: "top_center", until: "done",
                        png: #{ size: [80, 48] } });"#,
            crate::RunOptions {
                out_root: Some(root.clone()),
                ..Default::default()
            },
            |_| {},
            Default::default(),
        );
        assert!(s.ok, "{:?}", s.error);
        let m = Manifest::load(&sweep_folders(&root)[0]).unwrap();
        let k = m.find("w30").unwrap();
        assert_eq!(text(&m.rows[k], "rule"), "W30");
        assert_eq!(m.find("#2"), Some(k));
        assert_eq!(m.find("n=30"), Some(k));
        assert_eq!(m.find("W99"), None);
        // Favourite with an extra field, kept on a second add.
        let mut f = Favourites::load(&m.folder);
        let mut extra = Map::new();
        extra.insert("llm_note".into(), json!("nested triangles"));
        assert!(f.add(&m.rows[k], &extra));
        assert!(f.add(&m.rows[k], &Map::new()));
        f.save(&m.folder).unwrap();
        let back = Favourites::load(&m.folder);
        assert_eq!(back.items.len(), 1);
        assert_eq!(
            back.items.values().next().unwrap()["llm_note"],
            "nested triangles"
        );
        // Promotion with a title and note.
        let lib = jetcarp_store::Library::open(root.join("lib"));
        let id = promote_run_with(&m, k, &lib, Some("Rule thirty"), Some("chaotic")).unwrap();
        let finds = lib
            .quickstart(&[jetcarp_store::QsSource::Promotion])
            .unwrap();
        assert_eq!(finds.len(), 1);
        assert_eq!(finds[0].handle, format!("find:{id}"));
        let sc = &finds[0].scene;
        assert_eq!(sc.name, "Rule thirty");
        assert!(sc.description.starts_with("chaotic ("));
        // Contact sheet of two runs.
        let img = contact_sheet(
            &m,
            &[(0, vec!["A W28".into()]), (k, vec!["B W30".into()])],
            60,
            2,
            3,
        )
        .unwrap();
        assert!(img.width > 120 && img.height > 36);
        let _ = std::fs::remove_dir_all(&root);
    }
}

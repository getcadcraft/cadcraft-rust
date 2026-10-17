//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Price library from CSV (T9)", "Trade
//! packs: format, install, remove, export (TK.1)", the engine half of "Trade packs: the shipped packs (TK.2, TK.3)" and
//! "Trade packs: one example per pack, worked by hand". The shipped packs are read from the frozen 0.5 build
//! (`TAKEOFF_PACKS_DIR` overrides); those tests skip when it is absent. The twin-factor check of TK.2 reads the packs'
//! authoring sources (site tooling, not the engine) and is not ported.

use cc_takeoff::jv::{self, Obj};
use cc_takeoff::library as lb;
use cc_takeoff::model::{self, Takeoff};
use cc_takeoff::packs as pk;
use cc_takeoff::units as u;
use serde_json::{Value, json};
use std::path::PathBuf;

fn near(a: f64, b: f64, t: f64) -> bool {
    (a - b).abs() <= t * 1f64.max(a.abs()).max(b.abs())
}
fn o(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}
fn s(v: &Value) -> String {
    jv::stringify(v)
}

#[test]
fn price_library_from_csv() {
    assert_eq!(lb::parse_csv("\u{feff}a,\"b, c\",\"d \"\"q\"\"\"\r\n1,2,3\n\n"), vec![vec!["a", "b, c", "d \"q\""], vec!["1", "2", "3"]]);
    assert_eq!(lb::parse_csv("Item;Unit\nX;LF"), vec![vec!["Item", "Unit"], vec!["X", "LF"]]);
    let csv = [
        "Name,Type,UOM,Waste,Material,Labor,Height,Component,Formula",
        "Partition type A,Linear,LF,10%,$2.10,3.333,9'-0\",,",
        ",Component,EA,,4.25,,,Studs 16 o.c.,ceil(L / 1.333) + N",
        ",Component,sheets,10,14.98,22,,Gypsum board 4x8,ceil(A * 2 / 32)",
        "VCT flooring,,SF,5,1.85,1.10,,,",
        "Carpet,area,SY,,27,,,,",
        "Duplex outlets,Count,EA,,3.50,45,,,",
        "Mystery,Volume,CY,,1,,,,",
        ",Component,EA,,1,,,Orphan,N",
        "Bad rate,Linear,LF,,abc,,,,",
        "Wall in metres,Linear,m,,1,,2.7,,",
    ]
    .join("\n");
    let (entries, warnings) = lb::read_library_csv(&csv).unwrap();
    assert_eq!(entries.len(), 5, "{warnings:?}");
    let pa = &entries[0];
    assert!(
        pa["type"] == "linear"
            && pa["unit"] == "LF"
            && pa["waste"] == 1000.0
            && pa["mat"] == 21000.0
            && pa["lab"] == 33330.0
            && near(pa["heightFt"].as_f64().unwrap(), 9.0, 1e-9)
    );
    assert!(
        pa["components"].as_array().unwrap().len() == 2
            && pa["components"][0]["formula"] == "ceil(L / 1.333) + N"
            && pa["components"][1]["unit"] == "sheets"
            && pa["components"][1]["mat"] == 149800.0
    );
    assert!(entries[1]["type"] == "area" && entries[1]["unit"] == "SF");
    assert!(entries[2]["unit"] == "SY" && entries[3]["type"] == "count" && entries[3]["lab"] == 450000.0);
    assert!(
        entries
            .iter()
            .any(|e| e["name"] == "Wall in metres" && e["unit"] == "m" && near(e["heightFt"].as_f64().unwrap(), 2.7 / 0.3048, 1e-9))
    );
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(
        warnings.iter().any(|w| w.contains("row 8: type \"Volume\""))
            && warnings.iter().any(|w| w.contains("row 9: a component with no item"))
            && warnings.iter().any(|w| w.contains("row 10: Type an amount"))
    );
    assert!(lb::read_library_csv("Price,Unit\n1,LF").unwrap_err().contains("Item (or Name)"));
    let (back, _) = lb::read_library_csv(&lb::library_csv(&entries)).unwrap();
    assert_eq!(s(&Value::Array(back)), s(&Value::Array(entries.clone())));
    let mut t = Takeoff::default();
    let items: Vec<Value> = entries.iter().map(lb::item_from_entry).collect();
    let ids = t.add_items(&items, None).unwrap();
    assert!(ids.len() == 5 && t.items()[0]["components"].as_array().unwrap().len() == 2 && t.undo_stack.len() == 1);
    let mut vct = entries[1].clone();
    vct["name"] = json!("vct FLOORING");
    vct["mat"] = json!(20000);
    let mut tile = entries[1].clone();
    tile["name"] = json!("New tile");
    let (lib, added, updated) = lb::merge_entries(&entries, &[vct, tile]);
    assert!(added == 1.0 && updated == 1.0 && lib[1]["mat"] == 20000.0 && lib.len() == 6);
    let mut p2 = entries[0].clone();
    p2["mat"] = json!(25000);
    p2["heightFt"] = json!(12);
    let mut c0 = entries[0]["components"][0].clone();
    c0["mat"] = json!(50000);
    c0["formula"] = json!("L");
    p2["components"] = json!([c0]);
    let (lib2, _, _) = lb::merge_entries(&entries, &[p2]);
    let patches = lb::reprice_patches(t.items(), &lib2);
    assert!(patches.len() == 1 && patches[0].1["mat"] == 25000.0 && !patches[0].1.contains_key("heightFt"));
    t.update_items(&patches, None).unwrap();
    let pi = &t.items()[0];
    assert!(
        pi["mat"] == 25000.0
            && pi["components"][0]["mat"] == 50000.0
            && pi["components"][0]["formula"] == "ceil(L / 1.333) + N"
            && near(pi["heightFt"].as_f64().unwrap(), 9.0, 1e-9)
            && pi["components"][1]["mat"] == 149800.0
    );
    t.undo();
    assert_eq!(t.items()[0]["mat"], 21000.0);
    let stored = lb::read_stored(&json!([lb::entry_from_item(&t.items()[0]), { "name": "x", "type": "volume" }, null]));
    assert!(
        stored.len() == 1
            && stored[0]["components"].as_array().unwrap().len() == 2
            && stored[0].get("id").is_none()
            && stored[0].get("color").is_none()
    );
}

fn base_pack() -> Value {
    json!({
        "format": "takeoffcraft-pack", "version": 1, "id": "zz.test-pack", "name": "Test pack", "packVersion": "1.2.0", "license": "CC0-1.0", "status": "starter",
        "sources": [{ "id": "ga", "title": "GA-216", "publisher": "Gypsum Association", "url": "https://www.gypsum.org/" }, { "id": "bad", "title": "x", "url": "http://plain.example/" }],
        "items": [
            { "key": "wall", "name": "Partition A", "type": "linear", "unit": "LF", "heightFt": 9, "waste": 500, "mat": 99999, "lab": 12345, "csi": "09 21 16", "sources": ["ga", "nope"], "assume": ["Studs 16 in o.c."], "twin": "wall-m",
              "components": [{ "name": "Studs", "unit": "EA", "formula": "ceil(L*0.75)+N", "mat": 5000 }, { "name": "Broken", "unit": "EA", "formula": "L +" }, { "name": "Sneaky", "unit": "EA", "formula": "constructor(1)" }] },
            { "key": "wall-m", "name": "Partition A (metric)", "type": "linear", "unit": "m", "heightFt": 9, "twin": "wall", "components": [{ "name": "Studs", "unit": "EA", "formula": "ceil(L/0.4064)+N" }] },
            { "key": "outlet", "name": "Outlet", "type": "count", "unit": "EA", "components": [{ "name": "Box", "unit": "EA", "formula": "N" }] },
            { "key": "outlet", "name": "Duplicate key", "type": "count", "unit": "EA" },
            { "key": "BAD KEY", "name": "x", "type": "count" },
            { "key": "vol", "name": "Volume", "type": "volume" },
            null
        ]
    })
}
fn with(mut v: Value, k: &str, x: Value) -> Value {
    v[k] = x;
    v
}

#[test]
fn trade_pack_format() {
    let r = pk::read_pack(&base_pack()).unwrap();
    assert_eq!(r.pack["items"].as_array().unwrap().len(), 3, "{:?}", r.warnings);
    let w = &r.pack["items"][0];
    assert!(w["mat"] == 0.0 && w["lab"] == 0.0 && w["components"].as_array().unwrap().iter().all(|c| c["mat"] == 0.0 && c["lab"] == 0.0));
    assert!(w["components"].as_array().unwrap().len() == 1 && w["components"][0]["formula"] == "ceil(L*0.75)+N");
    assert!(w["csi"] == "09 21 16" && s(&w["sources"]) == "[\"ga\"]" && w["assume"][0] == "Studs 16 in o.c." && w["twin"] == "wall-m");
    assert!(r.pack["sources"].as_array().unwrap().len() == 2 && r.pack["sources"][1].get("url").is_none());
    assert!(
        r.warnings.iter().any(|x| x.contains("4 items could not be read")) && r.warnings.iter().any(|x| x.contains("2 assembly parts")),
        "{:?}",
        r.warnings
    );
    let muts: Vec<(Value, &str)> = vec![
        (with(base_pack(), "format", json!("takeoffcraft-project")), "not a TakeoffCraft trade pack"),
        (with(base_pack(), "version", json!(2)), "newer TakeoffCraft"),
        (with(base_pack(), "id", json!("Bad Id")), "id is missing"),
        (with(base_pack(), "id", json!("a..b")), "id is missing"),
        (with(base_pack(), "name", json!("")), "has no name"),
        (with(base_pack(), "packVersion", json!("v1")), "version number"),
        (with(base_pack(), "items", json!([{ "key": "x", "type": "count" }])), "no items that could be read"),
    ];
    for (j, why) in muts {
        let e = pk::read_pack(&j).unwrap_err();
        assert!(e.contains(why), "{e} / {why}");
    }
    assert!(pk::read_pack(&Value::Null).is_err() && pk::read_pack(&json!([])).is_err() && pk::read_pack(&json!("{}")).is_err());
    let rv = pk::read_pack(&with(base_pack(), "status", json!("reviewed"))).unwrap();
    assert!(rv.pack["status"] == "starter" && rv.warnings.iter().any(|x| x.contains("treated as a starter")));
    let mut b = with(base_pack(), "status", json!("reviewed"));
    b["reviewedBy"] = json!("A. Estimator");
    b["reviewedOn"] = json!("2026-11-01");
    let rv2 = pk::read_pack(&b).unwrap();
    assert!(rv2.pack["status"] == "reviewed" && rv2.pack["reviewedBy"] == "A. Estimator");
    assert_eq!(pk::read_pack(&with(base_pack(), "name", json!("Test\u{0}\u{7} pack"))).unwrap().pack["name"], "Test pack");
    let no_label = pk::read_pack(&with(
        base_pack(),
        "samples",
        json!({ "region": "Austin, TX", "date": "2026-10-01", "currency": "USD", "rates": { "wall": { "mat": 10000 } } }),
    ))
    .unwrap();
    assert!(no_label.pack.get("samples").is_none() && no_label.warnings.iter().any(|x| x.contains("sample rates were left out")));
    let sm = pk::read_pack(&with(
        base_pack(),
        "samples",
        json!({ "label": "Sample rates, test", "region": "Austin, TX", "date": "2026-10-01", "currency": "USD", "rates": { "wall": { "mat": 10000, "lab": 20000, "components": { "Studs": { "mat": 4000 }, "Nope": { "mat": 1 } } }, "ghost": { "mat": 1 } } }),
    ))
    .unwrap();
    let sr = &sm.pack["samples"]["rates"];
    assert!(sr["wall"]["mat"] == 10000.0 && sr.get("ghost").is_none() && sr["wall"]["components"].get("Nope").is_none());
    let plain = pk::pack_entries(&sm.pack, "imperial", false);
    assert!(
        plain.len() == 2
            && plain.iter().all(pk::rate_unset)
            && plain.iter().all(|e| e["pack"]["id"] == "zz.test-pack" && e["pack"]["version"] == "1.2.0")
    );
    assert!(
        jv::to_string(plain[0].get("src")).contains("GA-216, Gypsum Association")
            && jv::to_string(plain[0].get("note")).contains("Studs 16 in o.c.")
            && plain[0]["csi"] == "09 21 16"
    );
    let names: Vec<String> = pk::pack_entries(&sm.pack, "metric", false).iter().map(|e| jv::to_string(e.get("name"))).collect();
    assert_eq!(names.join("|"), "Partition A (metric)|Outlet");
    assert_eq!(pk::pack_entries(&sm.pack, "both", false).len(), 3);
    let ws = pk::pack_entries(&sm.pack, "imperial", true);
    assert!(
        ws[0]["mat"] == 10000.0
            && ws[0]["lab"] == 20000.0
            && ws[0]["components"][0]["mat"] == 4000.0
            && ws[0]["sample"]["region"] == "Austin, TX"
            && pk::rate_unset(&ws[1])
    );
    // install, again, own entries kept, remove
    let mine =
        json!({ "name": "Outlet", "type": "count", "unit": "EA", "heightFt": 0, "waste": 0, "mat": 350000, "lab": 0, "components": [] });
    let lib0 = vec![
        mine.clone(),
        json!({ "name": "Carpet", "type": "area", "unit": "SY", "heightFt": 0, "waste": 0, "mat": 270000, "lab": 0, "components": [] }),
    ];
    let (l1, a1, _, k1, _) = pk::install_pack(&lib0, &sm.pack, "imperial", false);
    assert!(a1 == 1.0 && k1 == 1.0 && l1.len() == 3 && l1[0] == mine);
    let (l2, a2, u2, _, _) = pk::install_pack(&l1, &sm.pack, "both", false);
    assert!(a2 == 1.0 && u2 == 1.0 && l2.len() == 4);
    assert_eq!(
        s(&Value::Array(pk::installed_packs(&l2))),
        s(&json!([{ "id": "zz.test-pack", "name": "Test pack", "version": "1.2.0", "count": 2 }]))
    );
    let (rm, removed) = pk::remove_pack(&l2, "zz.test-pack");
    assert!(removed == 2.0 && rm.len() == 2 && rm[0] == mine && pk::installed_packs(&rm).is_empty());
    let back = lb::read_stored(&Value::Array(l2.clone()));
    assert!(
        back.len() == 4
            && back[2]["pack"]["id"] == "zz.test-pack"
            && back[2]["csi"] == "09 21 16"
            && jv::to_string(back[2].get("src")).contains("GA-216")
    );
    let mut forged = mine.clone();
    forged["pack"] = json!({ "id": "Bad Id", "version": "x" });
    forged["src"] = json!("a\u{0}b");
    let f = lb::read_stored(&json!([forged]));
    assert!(f[0].get("pack").is_none() && f[0]["src"] == "a b");
    let mut tk = Takeoff::default();
    tk.add_items(&[lb::item_from_entry(&back[2])], None).unwrap();
    assert!(tk.items()[0]["csi"] == "09 21 16" && tk.items()[0]["components"].as_array().unwrap().len() == 1);
    // export your own library as a pack
    let mut m2 = mine.clone();
    m2["csi"] = json!("26 27 26");
    let own = vec![
        m2,
        json!({ "name": "Wall B", "type": "linear", "unit": "LF", "heightFt": 8, "waste": 1000, "mat": 21000, "lab": 33000, "components": [{ "name": "Studs", "unit": "EA", "formula": "ceil(L*0.75)+N", "waste": 0, "mat": 4250, "lab": 0 }] }),
    ];
    let xp = pk::library_pack(&own, &o(json!({ "id": "my-library", "name": "My shop", "author": "ACME" })));
    let xr = pk::read_pack(&xp).unwrap();
    assert!(
        xr.warnings.is_empty() && xr.pack["items"].as_array().unwrap().len() == 2 && xr.pack.get("samples").is_none(),
        "{:?}",
        xr.warnings
    );
    assert!(
        xr.pack["items"][1]["components"][0]["formula"] == "ceil(L*0.75)+N"
            && xr.pack["items"][0]["csi"] == "26 27 26"
            && near(xr.pack["items"][1]["heightFt"].as_f64().unwrap(), 8.0, 1e-9)
    );
    let xpr = pk::library_pack(
        &own,
        &o(
            json!({ "id": "my-library", "name": "My shop", "rates": true, "region": "Monterrey, NL", "date": "2026-10-14", "currency": "MXN" }),
        ),
    );
    let xrr = pk::read_pack(&xpr).unwrap();
    let sa = &xrr.pack["samples"];
    assert!(
        sa["region"] == "Monterrey, NL"
            && sa["currency"] == "MXN"
            && sa["rates"]["wall-b"]["mat"] == 21000.0
            && sa["rates"]["wall-b"]["components"]["Studs"]["mat"] == 4250.0
    );
    assert!(pk::library_pack(&own, &o(json!({ "rates": true }))).get("samples").is_none());
    let (ins, _, _, _, _) = pk::install_pack(&[], &xrr.pack, "both", true);
    assert!(ins[1]["mat"] == 21000.0 && ins[1]["components"][0]["mat"] == 4250.0 && ins[0]["mat"] == 350000.0);
}

fn packs_dir() -> Option<PathBuf> {
    let d = std::env::var_os("TAKEOFF_PACKS_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("/var/www/CADCRAFT/testfiles/fork-parity/js-0.5/site/public_html/assets/apps/takeoffcraft/dev/packs")
    });
    if d.join("index.json").exists() { Some(d) } else { None }
}
fn shipped() -> Option<Vec<(Value, Value, String)>> {
    let dir = packs_dir()?;
    let index: Value = serde_json::from_slice(&std::fs::read(dir.join("index.json")).ok()?).ok()?;
    let mut out = Vec::new();
    for meta in index["packs"].as_array()? {
        let text = std::fs::read_to_string(dir.join(meta["file"].as_str()?)).ok()?;
        out.push((meta.clone(), serde_json::from_str(&text).ok()?, text));
    }
    Some(out)
}

#[test]
fn shipped_packs() {
    let Some(list) = shipped() else {
        eprintln!("shipped packs not found: skipped");
        return;
    };
    assert!(list.len() >= 6);
    let banned = [
        "planswift",
        "bluebeam",
        "onscreen takeoff",
        "on-screen takeoff",
        "stack takeoff",
        "trimble",
        "accubid",
        "mc2",
        "sage estimating",
        "procore",
        "togal",
        "claude",
        "anthropic",
    ];
    for (meta, json, text) in &list {
        let r = pk::read_pack(json).unwrap();
        let items = r.pack["items"].as_array().unwrap();
        assert!(r.warnings.is_empty() && items.len() as f64 == meta["items"].as_f64().unwrap(), "{}: {:?}", meta["id"], r.warnings);
        assert!(r.pack["license"] == "CC0-1.0" && r.pack["status"] == "starter" && r.pack.get("samples").is_none());
        let low = text.to_lowercase();
        assert!(banned.iter().all(|b| !low.contains(b)), "{}", meta["id"]);
        assert!(items.iter().all(|i| i["sources"].as_array().is_some_and(|s| s.iter().any(|x| x != "geometry"))));
        assert!(items.iter().all(|i| i.get("csi").is_some_and(|c| !jv::to_string(Some(c)).is_empty())));
        assert!(items.iter().all(|i| !i["components"].as_array().unwrap().is_empty()));
        let key = |k: &Value| items.iter().find(|i| i["key"] == *k);
        let unpaired: Vec<&Value> = items
            .iter()
            .filter(|i| {
                pk::pack_system_of(i) != "any"
                    && !(i.get("twin").is_some()
                        && key(&i["twin"]).is_some_and(|t| t["twin"] == i["key"] && pk::pack_system_of(t) != pk::pack_system_of(i)))
            })
            .collect();
        assert!(unpaired.is_empty(), "{}: {unpaired:?}", meta["id"]);
    }
}

#[test]
fn one_example_per_pack() {
    let Some(list) = shipped() else {
        eprintln!("shipped packs not found: skipped");
        return;
    };
    let packs: Vec<(String, Value)> = list.iter().map(|(m, j, _)| (jv::to_string(m.get("id")), pk::read_pack(j).unwrap().pack)).collect();
    let hash = "e".repeat(64);
    let run = |id: &str, key: &str, marks: Vec<Value>| -> Option<model::Estimate> {
        let p = &packs.iter().find(|(i, _)| i == id)?.1;
        let name = p["items"].as_array()?.iter().find(|i| i["key"] == key)?["name"].clone();
        let e = pk::pack_entries(p, "both", false).into_iter().find(|x| x["name"] == name)?;
        let mut t = Takeoff::default();
        t.add_file(&o(json!({ "hash": hash, "name": "x.pdf", "size": 1, "pages": 1 })));
        let s0 = model::sheet_key(&hash, 0.0);
        t.set_scale(&s0, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": u::preset_ft_per_unit("a1/4", 1.0).unwrap(), "label": "1/4\"", "cal": null }), None).ok()?;
        let ids = t.add_items(&[lb::item_from_entry(&e)], None).ok()?;
        for mut m in marks {
            m["item"] = json!(ids[0]);
            m["sheet"] = json!(s0);
            t.add_mark(&o(m)).ok()?;
        }
        Some(model::estimate_all(&t.state))
    };
    let ft = |a: f64| a * 18.0;
    let runs = |lens: &[f64]| -> Vec<Value> {
        lens.iter()
            .enumerate()
            .map(|(i, l)| json!({ "pts": [{ "x": 0, "y": i as f64 * 100.0 }, { "x": ft(*l), "y": i as f64 * 100.0 }] }))
            .collect()
    };
    let rect = |w: f64, d: f64| {
        vec![json!({ "pts": [{ "x": 0, "y": 0 }, { "x": ft(w), "y": 0 }, { "x": ft(w), "y": ft(d) }, { "x": 0, "y": ft(d) }] })]
    };
    let counts = |n: usize| -> Vec<Value> { (0..n).map(|i| json!({ "pts": [{ "x": i as f64 * 10.0, "y": 0 }] })).collect() };
    let check = |label: &str, est: Option<model::Estimate>, want: &[(&str, f64, Option<f64>)]| {
        let est = est.unwrap_or_else(|| panic!("{label}: no pack / item"));
        let mut bad = Vec::new();
        for (name, v, vw) in want {
            let row = est.rows.iter().find(|r| model::rstr(r, "kind") == "component" && model::rstr(r, "name") == *name);
            match row {
                Some(r)
                    if r.get("error").is_none()
                        && near(model::rnum(r, "qty"), *v, 1e-6)
                        && vw.is_none_or(|w| near(model::rnum(r, "qtyW"), w, 1e-6)) => {}
                _ => {
                    bad.push(format!("{name}: {:?}", row.map(|r| (model::rnum(r, "qty"), model::rnum(r, "qtyW"), r.get("error").cloned()))))
                }
            }
        }
        assert!(bad.is_empty(), "{label}: {}", bad.join(" | "));
    };
    check(
        "drywall",
        run("trade.drywall-framing", "part-steel-12-1x1-10", runs(&[30.0, 12.0])),
        &[
            ("Steel track 3-5/8 in (top and bottom)", 84.0, Some(88.2)),
            ("Steel studs 3-5/8 in, cut to wall height", 34.0, None),
            ("Gypsum board 1/2 in, 4 x 8 ft sheets", 27.0, Some(29.7)),
            ("Drywall screws", 735.0, None),
            ("Joint tape", 315.0, None),
            ("Joint compound, ready-mixed", 8.4, None),
        ],
    );
    check(
        "drywall metric",
        run("trade.drywall-framing", "part-steel-12-1x1-10-m", runs(&[30.0, 12.0])),
        &[("Gypsum board 12.7 mm, 1220 x 2440 mm sheets", 27.0, None)],
    );
    check(
        "electrical EMT",
        run("trade.electrical", "emt-12-3", runs(&[25.0, 40.0])),
        &[
            ("EMT 1/2 in", 65.0, None),
            ("EMT couplings 1/2 in", 7.0, None),
            ("EMT connectors 1/2 in", 4.0, None),
            ("One-hole straps 1/2 in", 11.0, None),
            ("THHN #12", 201.0, Some(221.1)),
        ],
    );
    check(
        "electrical receptacles",
        run("trade.electrical", "recep-duplex-20", counts(12)),
        &[
            ("Device box, single gang", 12.0, None),
            ("Duplex receptacle 20 A", 12.0, None),
            ("Cover plate, single gang", 12.0, None),
            ("Ground pigtail with wire nut", 12.0, None),
        ],
    );
    check(
        "painting",
        run("trade.painting", "walls-new-9", runs(&[50.0])),
        &[("Primer, PVA / latex", 1.5, None), ("Interior latex, finish", 900.0 / 350.0, None)],
    );
    check(
        "concrete slab",
        run("trade.concrete", "sog-4-wwr", rect(30.0, 40.0)),
        &[
            ("Concrete", 400.0 / 27.0, Some(400.0 / 27.0 * 1.05)),
            ("Base course gravel 4 in", 400.0 / 27.0, None),
            ("Vapor retarder 6 mil", 1200.0, Some(1320.0)),
            ("Welded wire reinforcement 6x6 W1.4xW1.4", 1200.0, None),
            ("Edge forms", 140.0, None),
        ],
    );
    check(
        "concrete footing",
        run("trade.concrete", "ftg-20x10", runs(&[100.0])),
        &[
            ("Concrete", 100.0 * (20.0 / 12.0) * (10.0 / 12.0) / 27.0, None),
            ("Footing forms (both sides)", 2.0 * 100.0 * 10.0 / 12.0, None),
            ("Rebar #4", 200.0, None),
            ("Rebar #4 (weight)", 133.6, None),
        ],
    );
    check(
        "tile",
        run("trade.flooring", "tile-floor-conc", rect(12.0, 15.0)),
        &[("Floor tile", 180.0, Some(198.0)), ("Thin-set mortar, 50 lb bags", 2.0, None), ("Sanded grout", 14.4, None)],
    );
    check(
        "carpet",
        run("trade.flooring", "carpet-broadloom", rect(12.0, 15.0)),
        &[("Carpet", 20.0, Some(22.0)), ("Carpet pad", 20.0, None), ("Tack strip", 54.0, None)],
    );
    check(
        "brick",
        run("trade.masonry", "brick-veneer", rect(40.0, 10.0)),
        &[("Face brick, modular", 2700.0, Some(2835.0)), ("Mortar (type N)", 23.68, None), ("Veneer ties", 150.0, None)],
    );
    check(
        "block",
        run("trade.masonry", "cmu-8", rect(40.0, 10.0)),
        &[("Concrete block 8 x 8 x 16 in", 450.0, None), ("Joint reinforcement, ladder", 300.0, None)],
    );
    check(
        "shingles",
        run("trade.roofing-residential", "shingle-arch", rect(30.0, 40.0)),
        &[
            ("Shingles", 36.0, Some(39.6)),
            ("Synthetic underlayment", 1200.0, None),
            ("Drip edge, 10 ft lengths", 14.0, None),
            ("Starter strip", 140.0, None),
            ("Roofing nails 1-1/4 in", 3840.0, None),
        ],
    );
    check(
        "TPO",
        run("trade.roofing-commercial", "tpo-ma", rect(100.0, 50.0)),
        &[
            ("TPO membrane", 5000.0, Some(5500.0)),
            ("Polyiso insulation 4 x 8 ft", 157.0, None),
            ("Insulation fasteners and plates", 1256.0, None),
            ("Membrane fasteners and seam plates", 500.0, None),
        ],
    );
    check(
        "siding",
        run("trade.siding", "vinyl-d4", rect(40.0, 10.0)),
        &[
            ("Vinyl siding", 4.0, Some(4.4)),
            ("Siding nails", 450.0, None),
            ("Water-resistive barrier", 400.0, None),
            ("Cap fasteners", 200.0, None),
        ],
    );
    check(
        "base",
        run("trade.finish-carpentry", "base", runs(&[30.0, 18.0])),
        &[("Base molding", 48.0, Some(52.8)), ("Finish nails", 72.0, None)],
    );
    check(
        "doors",
        run("trade.finish-carpentry", "door-int", counts(6)),
        &[("Prehung interior door unit", 6.0, None), ("Door casing", 210.0, Some(231.0)), ("Lockset, passage or privacy", 6.0, None)],
    );
    check(
        "PVC",
        run("trade.plumbing", "pvc-dwv-4", runs(&[37.0, 13.0])),
        &[("PVC DWV pipe 4 in", 50.0, Some(55.0)), ("Pipe hangers 4 in", 15.0, None)],
    );
    check(
        "lavs",
        run("trade.plumbing", "lav", counts(4)),
        &[("Faucet", 4.0, None), ("Angle stops", 8.0, None), ("Supply lines", 8.0, None)],
    );
    check(
        "duct",
        run("trade.hvac", "rect-20x10", runs(&[64.0])),
        &[
            ("Galvanized sheet metal duct", 320.0, Some(368.0)),
            ("Duct hangers (pairs)", 9.0, None),
            ("Duct wrap 1-1/2 in", 320.0, Some(384.0)),
        ],
    );
    check(
        "batts",
        run("trade.insulation", "batt-r13-9", runs(&[120.0])),
        &[("Batt insulation", 1080.0, Some(1134.0)), ("Batt packages", 27.0, None)],
    );
    check(
        "bed",
        run("trade.landscape", "bed-mulch", rect(20.0, 15.0)),
        &[
            ("Planting soil 6 in", 300.0 * 6.0 / 12.0 / 27.0, None),
            ("Landscape fabric", 300.0, None),
            ("Fabric staples", 75.0, None),
            ("Mulch 3 in", 300.0 * 3.0 / 12.0 / 27.0, None),
        ],
    );
    check(
        "deck",
        run("trade.decking", "deck-wood-16", rect(12.0, 16.0)),
        &[
            ("Deck boards", 192.0 * 2.1333, None),
            ("Joists 2x8", 144.0, None),
            ("Deck screws", 615.0, None),
            ("Joist hangers", 24.0, None),
            ("Rim joist 2x8", 56.0, None),
        ],
    );
    check(
        "excavation",
        run("trade.general-contractor", "gc-excavation", rect(40.0, 50.0)),
        &[
            ("Excavation (bank)", 2000.0 * 3.0 / 27.0, None),
            ("Haul (loose)", 2000.0 * 3.0 / 27.0 * 1.25, None),
            ("Truck loads, 12 CY", 24.0, None),
        ],
    );
    check(
        "ceiling",
        run("trade.general-contractor", "gc-act-2x4", rect(20.0, 30.0)),
        &[
            ("Ceiling panels", 75.0, None),
            ("Main runners", 150.0, None),
            ("Cross tees 4 ft", 300.0, None),
            ("Wall angle", 100.0, None),
            ("Hanger wires", 38.0, None),
        ],
    );
    assert!(packs.len() >= 16 && packs.iter().all(|(id, _)| id.starts_with("trade.")));
}

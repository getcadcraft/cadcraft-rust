//! The native `.modelcraft` file: `Model.toJSON()` + the imported textures still in use (`imagesJSON`), as the app
//! saves it (without the camera and view style, which belong to the app's window, not the model).

use cc_m3d::Model;
use cc_m3d::json::Out;
use serde_json::Value;

use crate::Res;

/// The file text: `JSON.stringify({...model.toJSON(), images})`.
pub fn export_native(model: &mut Model) -> String {
    let mut o = Out::new(false);
    model.to_json_out(&mut o, false);
    let imgs = model.images_json();
    if !imgs.is_empty() && o.s.ends_with('}') {
        o.s.pop();
        o.raw(",\"images\":[");
        for (i, (id, im)) in imgs.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"id\":");
            o.str(id);
            o.raw(",\"name\":");
            o.str(&im.name);
            o.raw(",\"mime\":");
            o.str(&im.mime);
            o.raw(",\"data\":");
            o.str(&im.data);
            o.raw(",\"avg\":");
            match &im.avg {
                Some(a) => o.str(a),
                None => o.raw("null"),
            }
            o.ch('}');
        }
        o.raw("]}");
    }
    o.s
}

/// Open a file (untrusted): `model.setImages(j.images); model.load(j)`.
pub fn load_native(j: &Value) -> Res<Model> {
    let mut m = Model::new();
    m.set_images(j.get("images").unwrap_or(&Value::Null));
    m.load(j, false)?;
    Ok(m)
}

/// The model's file form for the parity harness: toJSON (non-finite numbers as the kit's markers) + the images.
pub fn model_canon(model: &mut Model) -> String {
    let mut o = Out::new(true);
    model.to_json_out(&mut o, false);
    let imgs = model.images.iter().map(|(id, im)| (id.clone(), im.clone())).collect::<Vec<_>>();
    if o.s.ends_with('}') {
        o.s.pop();
        o.raw(",\"images\":[");
        for (i, (id, im)) in imgs.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"id\":");
            o.str(id);
            o.raw(",\"name\":");
            o.str(&im.name);
            o.raw(",\"mime\":");
            o.str(&im.mime);
            o.raw(",\"data\":");
            o.str(&im.data);
            o.raw(",\"avg\":");
            match &im.avg {
                Some(a) => o.str(a),
                None => o.raw("null"),
            }
            o.ch('}');
        }
        o.raw("]}");
    }
    o.s
}

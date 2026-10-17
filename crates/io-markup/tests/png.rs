//! A small RGBA PNG decodes to its pixels (the picture markups embed it).
#[test]
fn small_png_decodes() {
    let src = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAACCAYAAAB/qH1jAAAAGUlEQVR4nGP4z8DwH4Sh1H8QqwGEgWJgDAD5Ng310r1TAwAAAABJRU5ErkJggg==";
    let info = cc_markup::imagestamp::image_info(src).expect("info");
    assert_eq!((info.width, info.height), (4, 2));
    let d = cc_io_markup::image::decode_png(&info.bytes).expect("decode");
    assert_eq!(d.2.len(), 32);
}

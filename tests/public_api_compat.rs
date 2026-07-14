use gerber_parse::coord::{GerberFileImage, Vec2I};

#[test]
fn legacy_module_paths_remain_available() {
    let point = Vec2I::new(1, 2);
    let item = gerber_parse::draw_item::DrawItem::new();
    let image = GerberFileImage::default();
    let export: fn(&GerberFileImage) -> serde_json::Value =
        gerber_parse::golden_export::export_golden_json;

    assert_eq!(point, gerber_parse::geometry::Vec2I::new(1, 2));
    assert_eq!(item.get_position(), Vec2I::new(0, 0));
    assert!(gerber_parse::gerber_parser::test_str_is_rs274(
        "%FSLAX24Y24*%\n%ADD10C,0.1*%\nD10*\nX10Y10D02*\nX20Y20D01*\nM02*\n"
    ));
    assert!(export(&image).is_object());
    assert_eq!(
        std::mem::size_of::<gerber_parse::x2_attribute::X2Attribute>(),
        std::mem::size_of::<gerber_parse::x2_gerber_attributes::X2Attribute>()
    );
}

use display_config::get_displays;

fn main() {
    let displays = get_displays().expect("Failed to get displays");

    for display in displays {
        println!("Display ID: {:?}", display.id);

        println!("  Origin: {:?}", display.origin);
        println!("  Size: {:?}", display.size);
        println!("  Is primary: {:?}", display.is_primary);
        println!("  Is mirrored: {:?}", display.is_mirrored);

        println!();
    }
}

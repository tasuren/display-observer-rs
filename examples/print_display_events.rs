use display_config::{DisplayEvent, DisplayObserver};

fn main() {
    let monitor = DisplayObserver::new().expect("Failed to create the instance");

    monitor.set_callback(|event| match event {
        DisplayEvent::Added { id, resolution } => {
            println!("Added: {id:?}, resolution = {resolution:?}")
        }
        DisplayEvent::Removed { id } => println!("Removed: {id:?}"),
        DisplayEvent::SizeChanged { id, before, after } => {
            println!("SizeChanged: {id:?}, before = {before:?}, after = {after:?}")
        }
        DisplayEvent::Mirrored { id } => println!("Mirrored: {id:?}"),
        DisplayEvent::UnMirrored { id } => println!("UnMirrored: {id:?}"),
    });

    monitor.run().expect("Failed to run the application");
}

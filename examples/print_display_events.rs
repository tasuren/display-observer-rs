use display_observer::{DisplayEvent, DisplayObserver};

fn main() {
    let monitor = DisplayObserver::new().expect("Failed to create the instance");

    monitor.set_callback(|event| match event {
        DisplayEvent::Added { id, resolution } => {
            println!("Display added: {id:?}, resolution = {resolution:?}")
        }
        DisplayEvent::Removed(id) => println!("Display removed: {id:?}"),
        DisplayEvent::ResolutionChanged { id, before, after } => {
            println!("Display resolution changed: {id:?}, before = {before:?}, after = {after:?}")
        }
    });

    monitor.run().expect("Failed to run the application");
}

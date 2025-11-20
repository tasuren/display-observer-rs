use display_observer::{DisplayEvent, DisplayObserver};

fn main() {
    let monitor = DisplayObserver::new().expect("Failed to create the instance.");

    monitor
        .set_callback(|event| match event {
            DisplayEvent::Added(id) => println!("Display added: {:?}", id),
            DisplayEvent::Removed(id) => println!("Display removed: {:?}", id),
            DisplayEvent::ResolutionChanged { id, resolution } => {
                println!(
                    "Display {id:?} changed resolution to {}x{}",
                    resolution.width, resolution.height
                )
            }
        })
        .expect("Failed to set the callback.");

    monitor.run().expect("Failed to run the application.");
}

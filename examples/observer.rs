use display_config::{DisplayObserver, MayBeDisplayAvailable};

fn on_event(event: MayBeDisplayAvailable) {
    match event {
        MayBeDisplayAvailable::Available { display, event } => {
            println!(
                "{event:?} ... id = {:?}, origin = {:?}, size = {:?}",
                display.id(),
                display.origin(),
                display.size()
            );
        }
        MayBeDisplayAvailable::NotAvailable { event } => {
            println!("{event:?} ... display is not available");
        }
    }
}

fn main() {
    let monitor = DisplayObserver::new().expect("Failed to create the instance");
    monitor.set_callback(on_event);
    monitor.run().expect("Failed to run the application");
}

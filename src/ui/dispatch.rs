/// Grand Central Dispatch (GCD) utilities for dispatching to the main thread
use dispatch::Queue;

/// Dispatch a closure to the main thread asynchronously
pub unsafe fn dispatch_to_main_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    Queue::main().exec_async(f);
}

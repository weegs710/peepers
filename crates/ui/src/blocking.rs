use iced::Task;

/// Runs `work` on a thread meant for blocking calls, so reading a slow file never stalls drawing.
pub fn run<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Task<T> {
    Task::future(smol::unblock(work))
}

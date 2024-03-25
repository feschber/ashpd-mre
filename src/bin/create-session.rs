use ashpd::desktop::input_capture::{Capabilities, InputCapture};
use std::os::fd::AsRawFd;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ashpd::Result<()> {
    let input_capture = InputCapture::new().await?;
    let (session, capabilities) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            Capabilities::Keyboard | Capabilities::Pointer | Capabilities::Touchscreen,
        )
        .await?;
    eprintln!("capabilities: {capabilities}");

    let eifd = input_capture.connect_to_eis(&session).await?;
    eprintln!("        eifd: {}", eifd.as_raw_fd());
    Ok(())
}

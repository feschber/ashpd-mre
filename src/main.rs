use ashpd::desktop::{input_capture::InputCapture, remote_desktop::RemoteDesktop};

#[tokio::main(flavor="current_thread")]
async fn main() {
    tokio::join!(ic(), rdp());
}

async fn ic() {
    let _ = InputCapture::new().await.unwrap();
}

async fn rdp() {
    let _ = RemoteDesktop::new().await.unwrap();
}

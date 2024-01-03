use ashpd::{desktop::{input_capture::{InputCapture, Capabilities}, remote_desktop::{RemoteDesktop, DeviceType}}, WindowIdentifier};

#[tokio::main(flavor="current_thread")]
async fn main() {
    tokio::join!(ic(), rdp());
}

async fn ic() {
    let input_capture = InputCapture::new()
        .await
        .unwrap();
    let (session, _) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            (Capabilities::Pointer | Capabilities::Keyboard).into()
        )
        .await
        .unwrap();
    input_capture
        .connect_to_eis(&session)
        .await
        .unwrap();
}

async fn rdp() {
    let remote_desktop = RemoteDesktop::new().await.unwrap();
    let session = remote_desktop.create_session().await.unwrap();

    remote_desktop
        .select_devices(
            &session,
            DeviceType::Pointer | DeviceType::Keyboard | DeviceType::Touchscreen,
        )
        .await
        .unwrap();

    remote_desktop
        .start(&session, &WindowIdentifier::default())
        .await
        .unwrap()
        .response()
        .unwrap();

    remote_desktop
        .connect_to_eis(&session)
        .await
        .unwrap();
}

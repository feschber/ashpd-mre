use anyhow::{anyhow, Result};
use ashpd::desktop::input_capture::{Barrier, Capabilities, InputCapture, Zones, Activated};
use futures::StreamExt;
use reis::{tokio::{EiConvertEventStream, EiEventStream}, ei};
use std::{collections::HashMap, os::unix::net::UnixStream};

use once_cell::sync::Lazy;

pub enum Position {
    Left,
    Right,
    Top,
    Bottom,
}


#[tokio::main(flavor="current_thread")]
async fn main() {
    ic().await.unwrap();
}

static INTERFACES: Lazy<HashMap<&'static str, u32>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("ei_connection", 1);
    m.insert("ei_callback", 1);
    m.insert("ei_pingpong", 1);
    m.insert("ei_seat", 1);
    m.insert("ei_device", 2);
    m.insert("ei_pointer", 1);
    m.insert("ei_pointer_absolute", 1);
    m.insert("ei_scroll", 1);
    m.insert("ei_button", 1);
    m.insert("ei_keyboard", 1);
    m.insert("ei_touchscreen", 1);
    m
});

fn select_barriers(zones: &Zones, pos: Position) -> Vec<Barrier> {
    zones
        .regions()
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let id = n as u32;
            let (x, y) = (r.x_offset(), r.y_offset());
            let (width, height) = (r.width() as i32, r.height() as i32);
            let barrier_pos = match pos {
                Position::Left => (x, y, x, y + height - 1), // start pos, end pos, inclusive
                Position::Right => (x + width - 1, y, x + width - 1, y + height - 1),
                Position::Top => (x, y, x + width - 1, y),
                Position::Bottom => (x, y + height - 1, x + width - 1, y + height - 1),
            };
            Barrier::new(id, barrier_pos)
        })
        .collect()
}

async fn ic() -> Result<()> {
    let input_capture = InputCapture::new()
        .await
        .unwrap();

    // create input capture session
    log::debug!("creating input capture session");
    let (session, _cap) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            (Capabilities::Keyboard | Capabilities::Pointer | Capabilities::Touchscreen).into(),
        )
        .await?;

    log::debug!("selecting zones");
    let zones = input_capture.zones(&session).await?.response()?;
    log::debug!("{zones:?}");
    // FIXME: position
    let barriers = select_barriers(&zones, Position::Left);

    log::debug!("selecting barriers: {barriers:?}");
    input_capture
        .set_pointer_barriers(&session, &barriers, zones.zone_set())
        .await?;

    // connect to eis server
    log::debug!("connect_to_eis");
    let fd = input_capture.connect_to_eis(&session).await?;

    log::debug!("enabling session");
    input_capture.enable(&session).await?;

    let mut activated = input_capture.receive_all_signals().await?;
    // let mut activated = input_capture.receive_activated().await?;

    // create unix stream from fd
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(true)?;

    // create ei context
    let context = ei::Context::new(stream)?;
    context.flush()?;

    let mut event_stream = EiEventStream::new(context.clone())?;
    let _handshake = match reis::tokio::ei_handshake(
        &mut event_stream,
        "lan-mouse",
        ei::handshake::ContextType::Receiver,
        &INTERFACES,
    ).await {
        Ok(res) => res,
        Err(e) => return Err(anyhow!("ei handshake failed: {e:?}")),
    };
    let mut event_stream = EiConvertEventStream::new(event_stream);

    loop {
        let activated: Activated = loop {
            log::debug!("receiving activation token ...");
            let signal = activated.next().await.ok_or(anyhow!("expected activate signal"))?;
            // break signal;
            log::info!("{signal:?}");
            if let Some(member) = signal.header().member() {
                if member == "Activated" {
                    let body = signal.body();
                    let activated: Activated = body.deserialize()
                        .expect("failed to deserialize body");
                    break activated;
                }
            }
        };
        log::info!("activated: {activated:?}");
        let mut i = 0;
        loop {
            let ei_event = event_stream.next().await.unwrap().unwrap();
            log::debug!("{ei_event:?}");

            /* just for debugging break out of the loop after 100 events */
            i += 1;
            if i == 100 {
                break;
            }
        }

        log::debug!("releasing input capture");
        input_capture.release(&session, activated.activation_id(), (100., 100.)).await.unwrap();
    }
}

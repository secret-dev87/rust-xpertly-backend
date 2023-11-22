use actix::prelude::*;
use actix_web_actors::ws;
use uuid::Uuid;
pub use xpertly_worker::WorkerLog;
use super::server::*;

pub struct WsActor {
    pub id: Uuid,
    pub srv_addr: Addr<LiveUpdateServer>,
}

impl Actor for WsActor {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        self.srv_addr
            .send(Subscribe {
                exe_id: self.id,
                client: addr.recipient()
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(SubscribeResult(session_id)) => {
                        act.id = session_id;
                    }
                    _ => ctx.stop(),
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        self.srv_addr.do_send(Unsubscribe {
            id: self.id,
        });
        Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        // socket is unidirectional, so we don't need to handle incoming message.
        // we only need to satisfy websocket protocol standards
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Pong(_)) => (),
            _ => (),
        }
    }
}

impl Handler<WorkerLog> for WsActor {
    type Result = ();

    fn handle(&mut self, msg: WorkerLog, ctx: &mut Self::Context) {
        println!("received worker log");
        ctx.text(serde_json::to_string(&msg).unwrap());
    }
}
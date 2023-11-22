use std::collections::{HashMap, HashSet};

use actix::prelude::*;
use actix_web_actors::ws;
use uuid::Uuid;
use xpertly_worker::{ WorkerLog, Publish };

type ClientSocket = Recipient<WorkerLog>;

#[derive(Message)]
#[rtype(result = "SubscribeResult")]
pub struct Subscribe {
    pub exe_id: Uuid,
    pub client: ClientSocket,
}

pub struct SubscribeResult(pub Uuid);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Unsubscribe {
    pub id: Uuid,
}

pub struct LiveUpdateServer {
    // collection of connected clients
    sessions: HashMap<Uuid, ClientSocket>,
    // mapping of execution ID to clients subscribed to that execution
    subscriptions: HashMap<Uuid, HashSet<Uuid>>,
    // buffered message of subscribers for execution ID
    buffered_message: HashMap<Uuid, Vec<WorkerLog>>,
}

impl LiveUpdateServer {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            subscriptions: HashMap::new(),
            buffered_message: HashMap::new(),
        }
    }
}

impl Actor for LiveUpdateServer {
    type Context = Context<Self>;
}

impl Handler<Subscribe> for LiveUpdateServer {
    type Result = MessageResult<Subscribe>;

    fn handle(&mut self, msg: Subscribe, _ctx: &mut Self::Context) -> Self::Result {
        let session_id = Uuid::new_v4();
        self.sessions.insert(session_id, msg.client.clone());
        self.subscriptions
            .entry(msg.exe_id.clone())
            .or_insert_with(HashSet::new)
            .insert(session_id);

        //send buffered message to subscriber
        if let Some(buffered_msg) = self.buffered_message.get(&msg.exe_id) {
            for buf_msg in buffered_msg {
                msg.client.do_send(buf_msg.clone());
            }
            self.buffered_message.remove(&msg.exe_id);
        }
        MessageResult(SubscribeResult(session_id))
    }
}

impl Handler<Unsubscribe> for LiveUpdateServer {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Self::Context) {
        self.sessions.remove(&msg.id);

        // remove client from any subscription it's a part of, 
        // also removing that subscription entirely if it's empty
        self.subscriptions.retain(|subscrib_uuid, clients| {
            clients.remove(&msg.id);
            //remove cbuffered message session when there is no clients subscribed to
            if clients.is_empty() == true {
                self.buffered_message.remove(&subscrib_uuid);
            }
            !clients.is_empty()
        });
    }
}

impl Handler<Publish> for LiveUpdateServer {
    type Result = ();

    fn handle(&mut self, msg: Publish, ctx: &mut Self::Context) -> Self::Result {
        println!("received publish message");
        match self.subscriptions.get(&msg.id) {
            Some(clients) => {
                for client in clients {
                    if let Some(client) = self.sessions.get(client) {
                        client.do_send(msg.msg.clone());
                    }
                }
            }
            _ => self
                .buffered_message
                .entry(msg.id.clone())
                .or_insert_with(Vec::new)
                .push(msg.msg.clone()),
        }
    }
}
use reqwest::{blocking::Client, redirect::Policy};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub(crate) struct Server {
    pub url: String,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    pub fn new(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/Fono.exe", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let thread = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap() == 0 {
                        break;
                    }
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        Self {
            url,
            requests,
            thread: Some(thread),
        }
    }

    pub fn requests(mut self) -> Vec<String> {
        self.thread.take().unwrap().join().unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

pub(crate) fn response(status: &str, headers: &str, data: &str) -> String {
    format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n{data}")
}

pub(crate) fn client() -> Client {
    // Compiled only for unit tests. Production clients require HTTPS and
    // origin URLs that have passed the GitHub manifest policy.
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}

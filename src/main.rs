use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::{env, thread};
use std::fs::{read, File};
use std::path::{Path, PathBuf};
use std::str::Split;
use flate2::Compression;
use flate2::write::GzEncoder;
use memchr::memmem;

fn get_request_start_line(request_line: &mut Split<&str>) -> Option<StartLine> {
    let mut parts = request_line.next().unwrap_or("").split_whitespace();

    Some(StartLine {
        method: parts.next().unwrap_or("").to_string(),
        target: parts.next().unwrap_or("").to_string(),
        version: parts.next().unwrap_or("").to_string(),
    })
}

fn get_request_headers(request_lines: &mut Split<&str>) -> Option<Headers> {
    let mut host: String = String::new();
    let mut user_agent: String = String::new();
    let mut accept: String = String::new();
    let mut accept_encoding: Vec<String> = Vec::new();
    let mut connection: String = String::new();
    let mut content_type: String = String::new();
    let mut content_length: usize = 0;

    // TODO: case-insensitive
    for line in request_lines {
        if line.starts_with("Host: ") {
            host = line[6..].to_string();
        } else if line.starts_with("User-Agent: ") {
            user_agent = line[12..].to_string();
        } else if line.starts_with("Accept: ") {
            accept = line[8..].to_string();
        }  else if line.starts_with("Accept-Encoding: ") {
            let mut encodings = line[17..].to_string().split(",").map(|s| s.trim().to_string()).collect::<Vec<String>>();
            accept_encoding.append(&mut encodings);
        }  else if line.starts_with("Connection: ") {
            connection = line[12..].to_string();
        } else if line.starts_with("Content-Type: ") {
            content_type = line[14..].to_string();
        } else if line.starts_with("Content-Length: ") {
            content_length = line[16..].parse::<usize>().unwrap_or(0);
        } else if line == "" {
            break;
        }
    }

    Some(Headers {
        host,
        user_agent,
        accept,
        accept_encoding,
        connection,
        content_type,
        content_length,
    })
}

fn get_file_response_format(path: &Path) -> String {
    let extension = path.extension().unwrap_or(std::ffi::OsStr::new(""));

    if extension == "html" {
        return String::from("text/html");
    } else if extension == "js" {
        return String::from("text/javascript");
    } else if extension == "css" {
        return String::from("text/css");
    }  else if extension == "txt" {
        return String::from("text/plain");
    } else if extension == "jpg" || extension == "jpeg" {
        return String::from("image/jpeg");
    } else if extension == "png" {
        return String::from("image/png");
    }

    return String::from("application/octet-stream");
}

fn controller(request: &Request, response: &mut Response, files_path: &Path) {
    let start = request.start.as_ref().expect("request.start should be Some");
    let headers = request.headers.as_ref().expect("request.headers should be Some");
    let body = request.body.as_ref().expect("request.body should be Some");

    if start.target == "/" {
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");

        return;
    }

    if start.target == "/user-agent" && start.method == "GET" {
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");
        response.body = headers.user_agent.clone().to_string().as_bytes().to_vec();

        return;
    }

    if start.method == "GET" && start.target.starts_with("/echo/") {
        let echo_message = &start.target[6..];
        response.status = String::from("200 OK");
        response.format = String::from("text/plain");
        response.body = echo_message.to_string().as_bytes().to_vec();

        return;
    }

    if start.method == "GET" && (start.target == "/files" || start.target.starts_with("/files/")) {
        // TODO: disallow absolut path and ../
        let file_path: PathBuf;
        if start.target == "/files" {
            file_path = files_path.join(&start.target[6..]).join("index.html");
        } else {
            file_path = files_path.join(&start.target[7..]);
        }

        println!("{}", file_path.display());

        if file_path.exists() && file_path.is_file() {
            response.status = String::from("200 OK");
            response.format = get_file_response_format(&file_path);
            response.body = read(file_path).unwrap().to_vec();

            return;
        }
    }

    if start.method == "POST" && headers.content_type == "application/octet-stream" && start.target.starts_with("/files/") {
        let file_path = files_path.join(&start.target[7..]);
        println!("{}", file_path.display());

        let mut file = File::create(file_path);
        file.unwrap().write_all(body);

        response.status = String::from("201 Created");

        return;
    }

    response.status = String::from("404 Not Found");
    response.format = String::from("text/plain");

    return;
}

struct StartLine {
    method: String,
    target: String,
    version: String,
}

struct Headers {
    host: String,
    user_agent: String,
    accept: String,
    accept_encoding: Vec<String>,
    connection: String,
    content_type: String,
    content_length: usize,
}

struct Request {
    start: Option<StartLine>,
    headers: Option<Headers>,
    body: Option<Vec<u8>>,
}

impl Request {
    fn print_headers(&self) {
        if let Some(start) = &self.start {
            println!("Start: {} {} {}", start.method, start.target, start.version);
        } else {
            println!("Start: <missing>");
        }

        if let Some(headers) = &self.headers {
            println!("Headers: {} {} {} {} {}", headers.host, headers.user_agent, headers.accept, headers.content_type, headers.content_length);
        } else {
            println!("Headers: <missing>");
        }
    }
}

struct Response {
    version: String,
    status: String,
    format: String,
    content_encoding: String,
    connection: String,
    body: Vec<u8>,
}

impl Response {
    fn build(&self) -> String {
        let mut headers = format!("{} {}\r\nContent-Type: {}\r\n", self.version, self.status, self.format);

        if self.content_encoding != "" {
            headers.push_str(&format!("Content-Encoding: {}\r\n", self.content_encoding));
        }

        if self.connection != "" {
            headers.push_str(&format!("Connection: {}\r\n", self.connection));
        }

        headers.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));

        headers
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let directory: String;
    if args.len() >= 3 && args[1] == "--directory" {
        directory = args[2].clone();
    } else {
        directory = env::current_dir().unwrap().join(PathBuf::from("files")).to_str().unwrap().to_string();
    }

    println!("Files path: {}", directory);

    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    for stream in listener.incoming() {
        let stream = stream.unwrap();
        let directory = directory.clone();

        thread::spawn(|| {
            handle_connection(stream, directory);
        });
    }
}

fn try_build_request(request: &mut Request, buffer: &mut Vec<u8>) -> Result<(), ()> {
    if request.start.is_none() && request.headers.is_none() {
        let body_start = match memmem::find(buffer.as_slice(), b"\r\n\r\n") {
            Some(pos) => pos + 4,
            None => return Err(()),
        };

        let body_buffer = buffer.split_off(body_start);
        let headers_buffer = std::mem::take(buffer);
        *buffer = body_buffer;

        let header_str = String::from_utf8_lossy(&headers_buffer);
        let mut header_read: Split<&str> = header_str.split("\r\n");

        request.start = get_request_start_line(&mut header_read);
        request.headers = get_request_headers(&mut header_read);
    }

    if request.start.is_some() && request.headers.is_some() && request.body.is_none() {
        let content_length = request.headers.as_ref().unwrap().content_length;

        if buffer.len() < content_length {
            return Err(());
        }

        let remaining = buffer.split_off(content_length);
        let body_buffer = std::mem::replace(buffer, remaining);
        request.body = Some(body_buffer);

        return Ok(());
    }

    if request.start.is_some() && request.headers.is_some() && request.body.is_some() {
        Ok(())
    } else {
        Err(())
    }
}

fn handle_connection(mut stream: TcpStream, directory: String) {
    println!("-- NEW CONNECTION --");
    println!("accepted new connection from {}", stream.peer_addr().unwrap());

    let mut conn_buffer: Vec<u8> = Vec::new();
    let mut request = Request {
        start: None,
        headers: None,
        body: None,
    };


    println!("reading from stream");

    loop {
        let mut buffer: [u8; 1024] = [0; 1024];
        let buffer_size = match stream.read(&mut buffer) {
            Ok(size) => size,
            Err(error) if error.kind() == ErrorKind::ConnectionAborted => {
                println!("read from stream was closed.");
                return;
            },
            Err(error) => panic!("{}", error),
        };

        conn_buffer = [conn_buffer, buffer[..buffer_size].to_vec()].concat();

        match try_build_request(&mut request, &mut conn_buffer) {
            Ok(_) => {
                match handle_request(&request, &mut stream, &directory) {
                    Ok(_) => {
                        request = Request {
                            start: None,
                            headers: None,
                            body: None,
                        };
                    },
                    Err(_) => break,
                };
            },
            Err(_) => {
                continue;
            }
        };
    }
}

fn handle_request(request: &Request, stream: &mut TcpStream, directory: &String) -> Result<(), ()> {
    println!("-- HANDLE NEW REQUEST --");
    request.print_headers();

    let mut response = Response {
        version: String::from("HTTP/1.1"),
        status: String::new(),
        format: String::new(),
        connection: String::new(),
        content_encoding: String::new(),
        body: Vec::new(),
    };

    controller(&request, &mut response, Path::new(&directory));

    let headers = request.headers.as_ref().unwrap();
    if headers.accept_encoding.contains(&String::from("gzip")) {
        response.content_encoding = String::from("gzip");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&response.body).unwrap();
        response.body = encoder.finish().unwrap();
    }

    if headers.connection == "close" {
        response.connection = String::from("close");
    }

    match stream.write_all(response.build().as_bytes()) {
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::Interrupted => {
            println!("write to stream was interrupted.");
            return Err(());
        },
        Err(error) => {
            println!("failed to write to stream: {}", error);
            return Err(());
        },
    }

    match stream.write_all(&response.body) {
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::Interrupted => {
            println!("write to stream was interrupted.");
            return Err(());;
        },
        Err(error) => {
            println!("failed to write to stream: {}", error);
            return Err(());
        },
    }

    if headers.connection == "close" {
        return Err(());
    }

    Ok(())
}

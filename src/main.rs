extern crate exoquant;
extern crate hyper;
extern crate hyper_rustls;
extern crate image;
extern crate kuchiki;
extern crate termion;
extern crate tokio;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate clap;
extern crate futures;
extern crate toml;

use clap::{App, Arg, SubCommand};
use exoquant::{optimizer, Color, Histogram, SimpleColorSpace};
use hyper::rt::{self, Future, Stream};
use hyper::{header, Body, Client, Method, Request, Uri};
use image::Pixel;
use kuchiki::traits::*;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use termion::color::{self, Rgb};

#[derive(Debug, Deserialize)]
struct Config {
    lights_endpoint: String,
}

fn main() {
    let matches = App::new("Daily Palette")
        .version("0.2")
        .about("grabs a daily image from the interwebs and pushes it to some hue lights")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("sets a custom config file")
                .default_value("banner.toml"),
        )
        .arg(Arg::with_name("push").short("p").long("push"))
        .subcommand(SubCommand::with_name("google").about("fetches from google's daily banner"))
        .subcommand(SubCommand::with_name("natgeo").about("fetches from natgeo's daily images"))
        .get_matches();

    let config_file = matches.value_of("config").unwrap_or("banner.toml");
    let conf: Config = {
        let conf_fd = File::open(config_file).unwrap();
        let mut conf_buf = BufReader::new(conf_fd);
        let mut contents: Vec<u8> = Vec::new();
        conf_buf.read_to_end(&mut contents).unwrap();
        toml::from_slice(&contents).unwrap()
    };
    let lights_url: Uri = conf.lights_endpoint.parse().unwrap();

    match matches.subcommand_name() {
        Some("google") => {
            let fut = google_banner_daily().map(|body| {
                let bytes: &[u8] = &body.into_bytes();
                let palette = img_to_palette(bytes);

                palette_to_terminal(&palette);
                palette
            });
            if matches.is_present("push") {
                let fut = fut.and_then(|palette| palette_to_lights(&palette, lights_url))
                    .map(|res| println!("status: {}", res.status()))
                    .map_err(|e| println!("something went wrong: {}", e));
                rt::run(fut);
            } else {
                let fut = fut.map(|_| ())
                    .map_err(|e| println!("something went wrong: {}", e));
                rt::run(fut);
            }
        }
        Some("natgeo") => {
            let fut = natgeo_daily().map(|body| {
                let bytes: &[u8] = &body.into_bytes();
                let palette = img_to_palette(bytes);

                palette_to_terminal(&palette);
                palette
            });
            if matches.is_present("push") {
                let fut = fut.and_then(|palette| palette_to_lights(&palette, lights_url))
                    .map(|res| println!("status: {}", res.status()))
                    .map_err(|e| println!("something went wrong: {}", e));
                rt::run(fut);
            } else {
                let fut = fut.map(|_| ())
                    .map_err(|e| println!("something went wrong: {}", e));
                rt::run(fut);
            }
        }
        None => println!("no option selected, bye"),
        Some(&_) => unimplemented!(),
    }
    // rt::run(change_lights_fut);
}

fn google_banner_daily() -> impl Future<Item = hyper::Chunk, Error = hyper::Error> {
    let css_selector = r#"meta[itemprop="image"]"#;
    let uri = "http://www.google.com/".parse().unwrap();
    let client = Client::new();
    let fut = client
        .get(uri)
        .and_then(|res| res.into_body().concat2())
        .and_then(move |body| {
            let body: &[u8] = &body.into_bytes();
            let body = String::from_utf8_lossy(&body.to_vec()).into_owned();

            let document = kuchiki::parse_html().one(body);
            let node = {
                let node = document.select_first(css_selector);
                if node.is_err() {
                    println!("no selector match");
                }
                node.unwrap()
            };
            let node = node.as_node().as_element().unwrap();
            let attrs = node.attributes.borrow();
            let content = attrs.get("content").unwrap().to_owned();

            let img_uri: Uri = format!("http://www.google.com/{}", content)
                .parse()
                .unwrap();

            client.get(img_uri)
        })
        .and_then(|res| res.into_body().concat2());
    fut
}

fn img_to_palette(img: &[u8]) -> Vec<Color> {
    let img = image::load_from_memory(img).unwrap();
    let rgba = match img {
        image::DynamicImage::ImageRgba8(pxls) => pxls,
        _ => img.to_rgba(),
    };
    let histogram: Histogram = rgba.pixels()
        .filter_map(|px| {
            let (r, g, b, a) = px.channels4();
            if a > 128 {
                Some(Color::new(r, g, b, a))
            } else {
                None
            }
        })
        .collect();
    let palette = exoquant::generate_palette(
        &histogram,
        &SimpleColorSpace::default(),
        &optimizer::KMeans,
        4,
    );
    palette
}

fn palette_to_terminal(palette: &Vec<Color>) {
    for c in palette.iter() {
        println!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    }

    for c in palette.iter() {
        print!("{}██████", color::Fg(Rgb(c.r, c.g, c.b)));
    }
    print!("{}\n", color::Fg(color::Reset));
}

fn palette_to_lights(palette: &Vec<Color>, lights_url: Uri) -> Box<hyper::client::ResponseFuture> {
    let https = hyper_rustls::HttpsConnector::new(4);
    let client = Client::builder().build(https);

    let color1 = palette.get(0).unwrap();
    let color2 = palette.get(1).unwrap();
    let payload = json!({
                "color1": format!("#{:02x}{:02x}{:02x}", color1.r, color1.g, color1.b),
                "color2": format!("#{:02x}{:02x}{:02x}", color2.r, color2.g, color2.b),
            });

    let mut req = Request::new(Body::from(payload.to_string()));
    *req.uri_mut() = lights_url;
    *req.method_mut() = Method::POST;
    req.headers_mut().insert(
        "content-type",
        header::HeaderValue::from_str("application/json").unwrap(),
    );

    let fut = client.request(req);
    Box::new(fut)
}

fn natgeo_daily() -> impl Future<Item = hyper::Chunk, Error = hyper::Error> {
    let selector = r#"meta[name="image"][property="og:image"]"#;
    let uri = "http://yourshot.nationalgeographic.com/daily-dozen/"
        .parse()
        .unwrap();
    let client = Client::new();

    let fut = client
        .get(uri)
        .and_then(|res| res.into_body().concat2())
        .map(move |body| {
            let body: &[u8] = &body.into_bytes();
            let body = String::from_utf8_lossy(&body).into_owned();

            let document = kuchiki::parse_html().one(body);
            let node = {
                let node = document.select_first(selector);
                if node.is_err() {
                    println!("no selector match");
                }
                node.unwrap()
            };
            let attrs = node.as_node().as_element().unwrap().attributes.borrow();
            let content_uri: Uri = attrs.get("content").unwrap().parse().unwrap();
            content_uri
        })
        .and_then(move |daily_img_uri| {
            println!("image uri: {}", daily_img_uri);
            client.get(daily_img_uri)
        })
        .and_then(|res| res.into_body().concat2());
    fut
}

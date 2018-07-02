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
extern crate toml;

use exoquant::{optimizer, Color, Histogram, SimpleColorSpace};
use hyper::rt::{self, Future, Stream};
use hyper::{header, Body, Client, Method, Request};
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
    let css_selector = r#"meta[itemprop="image"]"#;
    let uri = "http://www.google.com/".parse().unwrap();
    let https = hyper_rustls::HttpsConnector::new(4);
    let client = Client::builder().build(https);
    let lights_client = client.clone();

    let conf: Config = {
        let conf_fd = File::open("banner.toml").unwrap();
        let mut conf_buf = BufReader::new(conf_fd);
        let mut contents: Vec<u8> = Vec::new();
        conf_buf.read_to_end(&mut contents).unwrap();
        toml::from_slice(&contents).unwrap()
    };
    let lights_url = conf.lights_endpoint.parse().unwrap();

    let goog_daily_banner_uri_future = client
        .get(uri)
        .and_then(|res| res.into_body().concat2())
        .map(move |body| {
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

            format!("http://www.google.com/{}", content)
        });

    let banner_img_fut = goog_daily_banner_uri_future
        .and_then(move |daily_banner| {
            {
                println!("banner url: {}", daily_banner);
            }
            let banner_url = daily_banner.parse().unwrap();
            client.get(banner_url)
        })
        .and_then(|res| res.into_body().concat2())
        .map(|body| {
            let body = &body.into_bytes();
            let img = image::load_from_memory(body).unwrap();
            let histogram: Histogram = img.as_rgba8()
                .unwrap()
                .pixels()
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
                5,
            );
            for c in palette.iter() {
                println!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
            }

            for c in palette.iter() {
                print!("{}██████", color::Fg(Rgb(c.r, c.g, c.b)));
            }
            print!("{}\n", color::Fg(color::Reset));

            palette
        });

    let change_lights_fut = banner_img_fut
        .and_then(move |palette| {
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

            lights_client.request(req)
        })
        .map(|resp| println!("status: {}", resp.status()))
        .map_err(|err| println!("error while changing lights: {}", err));

    rt::run(change_lights_fut);
}

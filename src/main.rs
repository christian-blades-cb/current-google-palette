extern crate exoquant;
extern crate hyper;
extern crate hyper_rustls;
extern crate image;
extern crate kuchiki;
extern crate termion;
extern crate tokio;

use exoquant::{optimizer, Color, Histogram, SimpleColorSpace};
use hyper::Client;
use hyper::rt::{self, Future, Stream};
use image::Pixel;
use kuchiki::traits::*;
use termion::color::{self, Rgb};

fn main() {
    let css_selector = r#"meta[itemprop="image"]"#;
    let uri = "http://www.google.com/".parse().unwrap();
    let client = Client::new();

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
        })
        .map_err(|err| println!("Error: {}", err));

    rt::run(banner_img_fut);
}

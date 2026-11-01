//! Derives a design system from a [`Scan`]. Everything here traces to something the page showed:
//! a role, token or semantic color with nothing behind it is left out, never defaulted.

mod colors;
mod components;
mod layout;
#[cfg(test)]
mod tests;
mod typography;
mod values;

use serde::Serialize;

use super::color::Rgba;
use super::scan::Scan;
use colors::derive_colors;
use components::derive_components;
use layout::{derive_radius, derive_shadows, derive_spacing};
use typography::derive_type;

pub use values::parse_box_shadow;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ramp {
    pub role: String,
    pub base: Rgba,
    pub steps: [Rgba; 11],
    pub uses: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeToken {
    pub family: String,
    pub size: f64,
    pub weight: u16,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComponentToken {
    pub name: String,
    pub props: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Facts {
    pub primary_buttons: u32,
    pub primary_links: u32,
    pub smallest_text: f64,
    pub dark: bool,
    pub colors_seen: usize,
    pub body: Option<TypeToken>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tokens {
    pub site: String,
    pub url: String,
    pub date: String,
    pub palette: Vec<Ramp>,
    pub semantic: Vec<(String, Rgba)>,
    pub families: Vec<(String, String)>,
    pub type_roles: Vec<(String, TypeToken)>,
    pub type_scale: Vec<(String, f64)>,
    pub spacing_base: Option<f64>,
    pub spacing: Vec<(String, f64)>,
    pub radius: Vec<(String, String)>,
    pub shadows: Vec<(String, String)>,
    pub breakpoints: Vec<u32>,
    pub variables: Vec<(String, String)>,
    pub components: Vec<ComponentToken>,
    pub facts: Facts,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Rgba,
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub inset: bool,
}

const MAX_VARIABLES: usize = 200;

/// `https://www.apple.com/x` → `Apple`; `localhost:5173` → `Localhost`.
pub fn site_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.trim_start_matches('[').split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    let is_ip = labels.len() == 4 && labels.iter().all(|l| l.parse::<u8>().is_ok());
    let n = labels.len();
    let name = if is_ip || n < 2 {
        host
    } else if n >= 3
        && labels[n - 1].len() == 2
        && ["co", "com", "org", "net", "gov", "ac", "edu"].contains(&labels[n - 2])
    {
        labels[n - 3]
    } else {
        labels[n - 2]
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn derive(scan: &Scan, date: &str) -> Tokens {
    let colors = derive_colors(scan);
    let text = derive_type(scan);
    let (spacing_base, spacing) = derive_spacing(scan);
    let radius = derive_radius(scan);
    let mut variables = scan.variables.clone();
    variables.sort_by(|a, b| a.0.cmp(&b.0));
    variables.truncate(MAX_VARIABLES);
    let mut breakpoints: Vec<u32> = scan
        .breakpoints
        .iter()
        .copied()
        .filter(|b| *b > 0)
        .collect();
    breakpoints.sort_unstable();
    breakpoints.dedup();

    let mut t = Tokens {
        site: site_name(&scan.url),
        url: scan.url.clone(),
        date: date.to_string(),
        palette: colors.palette,
        semantic: colors.semantic,
        families: text.families,
        type_roles: text.roles,
        type_scale: text.scale,
        spacing_base,
        spacing,
        radius,
        shadows: derive_shadows(scan),
        breakpoints,
        variables,
        components: Vec::new(),
        facts: Facts {
            primary_buttons: colors.primary_buttons,
            primary_links: colors.primary_links,
            smallest_text: text.smallest,
            dark: colors.dark,
            colors_seen: colors.seen,
            body: text.body,
        },
    };
    t.components = derive_components(scan, &t);
    t
}

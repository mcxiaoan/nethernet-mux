use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "nethernet-mux.json";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostType {
    Ipv4,
    Ipv6,
    Domain,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    /// BDS service host type.(ipv4/ipv6/domain)
    pub bds_type: HostType,
    /// BDS signaling service host name or IP, excluding ports.
    pub bds_host: String,
    /// the TCP port for BDS signaling service.
    pub bds_tcp: u16,
    /// the TCP listening port used by the player to connect to the proxy.
    pub listen_tcp: u16,
    /// the UDP listening port shared by players for game data.
    pub listen_mux: u16,
    /// SDP candidate external type.(ipv4/ipv6/domain)
    pub public_type: HostType,
    /// SDP candidate external host.(Compatible Domain/IPv4/IPv6)
    pub public_host: String,
    /// SDP candidate external port.
    pub public_port: u16,
}

pub fn initial_config() -> Result<Vec<Config>, Box<dyn Error>> {
    // read the path where the file is located
    let file: PathBuf = std::env::current_dir()?.join(CONFIG_FILE);
    // determine if the file exists
    if !file.exists() {
        create_empty_config(&file)?;
    }
    // read the contents of the config file
    let contents = fs::read_to_string(&file)
        .map_err(|error| format!("Failed to read config file '{}': {}", file.display(), error))?;
    // determine if the content is empty
    if contents.trim().is_empty() {
        create_empty_config(&file)?;
    }
    // parse the JSON contents into config structs
    let configs: Vec<Config> = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "Failed to parse config file '{}': {}\nPlease check the JSON syntax and config structure.",
            file.display(),
            error
        )
    })?;
    //determine if the config quantity is empty
    if configs.is_empty() {
        return Err("config is empty".into());
    }
    validate_configs(&configs)?;
    Ok(configs)
}

fn create_empty_config(file: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(file, "[]").map_err(|error| {
        format!(
            "Failed to create config file '{}': {}",
            file.display(),
            error
        )
    })?;
    Err(format!("config is empty, please configure: {}", file.display()).into())
}

fn validate_configs(configs: &[Config]) -> Result<(), Box<dyn Error>> {
    let mut ports: HashSet<u16> = HashSet::new();
    for (index, config) in configs.iter().enumerate() {
        // validate individual config
        validate_config(index, config)?;
        let instance = index + 1;
        // check listen_tcp
        if !ports.insert(config.listen_tcp) {
            return Err(format!(
                "BDS instance {}: listen_tcp port {} is already in use",
                instance, config.listen_tcp
            )
            .into());
        }
        // check listen_mux
        if !ports.insert(config.listen_mux) {
            return Err(format!(
                "BDS instance {}: listen_mux port {} is already in use",
                instance, config.listen_mux
            )
            .into());
        }
    }
    Ok(())
}

fn validate_config(index: usize, config: &Config) -> Result<(), Box<dyn Error>> {
    // validate individual config
    validate_host(index, "bds_host", config.bds_type, &config.bds_host)?;
    validate_port(index, "bds_tcp", config.bds_tcp)?;
    validate_port(index, "listen_tcp", config.listen_tcp)?;
    validate_port(index, "listen_mux", config.listen_mux)?;
    validate_host(
        index,
        "public_host",
        config.public_type,
        &config.public_host,
    )?;
    validate_port(index, "public_port", config.public_port)?;
    Ok(())
}

fn validate_domain(domain: &str) -> bool {
    let domain = domain.strip_suffix('.').unwrap_or(domain);
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
    })
}

fn validate_host(
    index: usize,
    field: &str,
    host_type: HostType,
    host: &str,
) -> Result<(), Box<dyn Error>> {
    let valid = match host_type {
        HostType::Ipv4 => {
            matches!(host.parse::<IpAddr>(), Ok(IpAddr::V4(_)))
        }
        HostType::Ipv6 => {
            matches!(host.parse::<IpAddr>(), Ok(IpAddr::V6(_)))
        }
        HostType::Domain => validate_domain(host),
    };
    if !valid {
        let expected = match host_type {
            HostType::Ipv4 => "IPv4",
            HostType::Ipv6 => "IPv6",
            HostType::Domain => "domain name",
        };
        return Err(format!(
            "BDS instance {}: {} must be a valid {}",
            index + 1,
            field,
            expected
        )
        .into());
    }

    Ok(())
}

fn validate_port(index: usize, field: &str, port: u16) -> Result<(), Box<dyn Error>> {
    if port == 0 {
        return Err(format!("BDS instance {}: {} cannot be 0", index + 1, field).into());
    }
    Ok(())
}

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ckb_sdk::{
    wallet::{Key, KeyStore, MasterPrivKey},
    Address, NetworkType,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use numext_fixed_hash::{H160, H256};

use super::CliSubCommand;
use crate::utils::{
    arg_parser::{
        ArgParser, DurationParser, ExtendedPrivkeyPathParser, FixedHashParser, PrivkeyPathParser,
    },
    other::read_password,
    printer::{OutputFormat, Printable},
};

pub struct AccountSubCommand<'a> {
    key_store: &'a mut KeyStore,
}

impl<'a> AccountSubCommand<'a> {
    pub fn new(key_store: &'a mut KeyStore) -> AccountSubCommand<'a> {
        AccountSubCommand { key_store }
    }

    pub fn subcommand(name: &'static str) -> App<'static, 'static> {
        let arg_lock_arg = Arg::with_name("lock-arg")
            .long("lock-arg")
            .takes_value(true)
            .validator(|input| FixedHashParser::<H160>::default().validate(input))
            .required(true)
            .help("The lock_arg (identifier) of the account");
        let arg_privkey_path = Arg::with_name("privkey-path")
            .long("privkey-path")
            .takes_value(true);
        let arg_extended_privkey_path = Arg::with_name("extended-privkey-path")
            .long("extended-privkey-path")
            .takes_value(true)
            .help("Extended private key path (include master private key and chain code)");
        SubCommand::with_name(name)
            .about("Management accounts")
            .subcommands(vec![
                SubCommand::with_name("list").about("List all accounts"),
                SubCommand::with_name("new").about("Creates a new account and prints related information."),
                SubCommand::with_name("import")
                    .about("Imports an unencrypted private key from <privkey-path> and creates a new account.")
                    .arg(
                        arg_privkey_path
                            .clone()
                            .required_unless("extended-privkey-path")
                            .validator(|input| PrivkeyPathParser.validate(input))
                            .help("The privkey is assumed to contain an unencrypted private key in hexadecimal format. (only read first line)")
                    )
                    .arg(arg_extended_privkey_path
                         .clone()
                         .required_unless("privkey-path")
                         .validator(|input| ExtendedPrivkeyPathParser.validate(input))
                    ),
                SubCommand::with_name("unlock")
                    .about("Unlock an account")
                    .arg(arg_lock_arg.clone())
                    .arg(
                        Arg::with_name("keep")
                            .long("keep")
                            .takes_value(true)
                            .validator(|input| DurationParser.validate(input))
                            .default_value("30m")
                            .help("How long before the key expired (repeat unlock will increase the time)")
                    ),
                SubCommand::with_name("update")
                    .about("Update password of an account")
                    .arg(arg_lock_arg.clone()),
                SubCommand::with_name("export")
                    .about("Export master private key and chain code as hex plain text (USE WITH YOUR OWN RISK)")
                    .arg(arg_lock_arg.clone())
                    .arg(
                        arg_extended_privkey_path
                            .clone()
                            .required(true)
                            .help("Output extended private key path (PrivKey + ChainCode)")
                    ),
            ])
    }
}

impl<'a> CliSubCommand for AccountSubCommand<'a> {
    fn process(
        &mut self,
        matches: &ArgMatches,
        format: OutputFormat,
        color: bool,
    ) -> Result<String, String> {
        match matches.subcommand() {
            ("list", _) => {
                let mut accounts = self
                    .key_store
                    .get_accounts()
                    .iter()
                    .map(|(address, filepath)| (address.clone(), filepath.clone()))
                    .collect::<Vec<(H160, PathBuf)>>();
                accounts.sort_by(|a, b| a.1.cmp(&b.1));
                let resp = accounts
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (lock_arg, filepath))| {
                        let address = Address::from_lock_arg(&lock_arg[..]).unwrap();
                        let timeout = self.key_store.get_lock_timeout(&lock_arg);
                        let status = timeout
                            .map(|timeout| format!("lock after: {}", timeout))
                            .unwrap_or_else(|| "locked".to_owned());
                        serde_json::json!({
                            "#": idx,
                            "lock_arg": format!("{:x}", lock_arg),
                            "address": {
                                "mainnet": address.to_string(NetworkType::MainNet),
                                "testnet": address.to_string(NetworkType::TestNet),
                            },
                            "path": filepath.to_string_lossy(),
                            "status": status,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!(resp).render(format, color))
            }
            ("new", _) => {
                println!("Your new account is locked with a password. Please give a password. Do not forget this password.");

                let pass = read_password(true, None)?;
                let lock_arg = self
                    .key_store
                    .new_account(pass.as_bytes())
                    .map_err(|err| err.to_string())?;
                let address = Address::from_lock_arg(&lock_arg[..]).unwrap();
                let resp = serde_json::json!({
                    "lock_arg": format!("{:x}", lock_arg),
                    "address": {
                        "mainnet": address.to_string(NetworkType::MainNet),
                        "testnet": address.to_string(NetworkType::TestNet),
                    },
                });
                Ok(resp.render(format, color))
            }
            ("import", Some(m)) => {
                let secp_key: Option<secp256k1::SecretKey> =
                    PrivkeyPathParser.from_matches_opt(m, "privkey-path", false)?;
                let password = read_password(true, None)?;
                let lock_arg = if let Some(secp_key) = secp_key {
                    self.key_store
                        .import_secp_key(&secp_key, password.as_bytes())
                        .map_err(|err| err.to_string())?
                } else {
                    let master_privkey: MasterPrivKey =
                        ExtendedPrivkeyPathParser.from_matches(m, "extended-privkey-path")?;
                    let key = Key::new(master_privkey);
                    self.key_store
                        .import_key(&key, password.as_bytes())
                        .map_err(|err| err.to_string())?
                };
                let address = Address::from_lock_arg(&lock_arg[..]).unwrap();
                let resp = serde_json::json!({
                    "lock_arg": format!("{:x}", lock_arg),
                    "address": {
                        "mainnet": address.to_string(NetworkType::MainNet),
                        "testnet": address.to_string(NetworkType::TestNet),
                    },
                });
                Ok(resp.render(format, color))
            }
            ("unlock", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let keep: Duration = DurationParser.from_matches(m, "keep")?;
                let password = read_password(false, None)?;
                let lock_after = self
                    .key_store
                    .timed_unlock(&lock_arg, password.as_bytes(), keep)
                    .map(|timeout| timeout.to_string())
                    .map_err(|err| err.to_string())?;
                let resp = serde_json::json!({
                    "lock-after": lock_after,
                });
                Ok(resp.render(format, color))
            }
            ("update", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let old_password = read_password(false, Some("Old password"))?;
                let new_passsword = read_password(true, Some("New password"))?;
                self.key_store
                    .update(&lock_arg, old_password.as_bytes(), new_passsword.as_bytes())
                    .map_err(|err| err.to_string())?;
                Ok("success".to_owned())
            }
            ("export", Some(m)) => {
                let lock_arg: H160 =
                    FixedHashParser::<H160>::default().from_matches(m, "lock-arg")?;
                let key_path = m.value_of("extended-privkey-path").unwrap();
                let password = read_password(false, None)?;

                if Path::new(key_path).exists() {
                    return Err(format!("File exists: {}", key_path));
                }
                let master_privkey = self
                    .key_store
                    .export_key(&lock_arg, password.as_bytes())
                    .map_err(|err| err.to_string())?;
                let bytes = master_privkey.to_bytes();
                let privkey = H256::from_slice(&bytes[0..32]).unwrap();
                let chain_code = H256::from_slice(&bytes[32..64]).unwrap();
                let mut file = fs::File::create(key_path).map_err(|err| err.to_string())?;
                file.write(format!("{:x}\n", privkey).as_bytes())
                    .map_err(|err| err.to_string())?;
                file.write(format!("{:x}", chain_code).as_bytes())
                    .map_err(|err| err.to_string())?;
                Ok(format!(
                    "Success exported account as extended privkey to: \"{}\", please use this file carefully",
                    key_path
                ))
            }
            _ => Err(matches.usage().to_owned()),
        }
    }
}

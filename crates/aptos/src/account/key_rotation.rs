// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use crate::common::{
    types::{
        CliCommand, CliConfig, CliError, CliTypedResult, ConfigSearchMode, EncodingOptions,
        EncodingType, ExtractPublicKey, ParsePrivateKey, ProfileConfig, ProfileOptions,
        PublicKeyInputOptions, RestOptions, RotationProofChallenge, TransactionOptions,
        TransactionSummary,
    },
    utils::{prompt_yes_with_override, read_line},
};
use aptos_crypto::{
    ed25519::{Ed25519PrivateKey, Ed25519PublicKey},
    PrivateKey, SigningKey,
};
use aptos_rest_client::Client;
use aptos_types::{
    account_address::AccountAddress, account_config::CORE_CODE_ADDRESS,
    transaction::authenticator::AuthenticationKey,
};
use async_trait::async_trait;
use cached_packages::aptos_stdlib;
use clap::Parser;
use serde::{Deserialize, Serialize};

/// Command to rotate an account's authentication key
///
#[derive(Debug, Parser)]
pub struct RotateKey {
    #[clap(flatten)]
    pub(crate) txn_options: TransactionOptions,

    /// File name that contains the new private key
    #[clap(long, group = "private_key_to_rotate_to", parse(from_os_str))]
    pub(crate) new_private_key_file: Option<PathBuf>,
    /// New private key encoded in a type as shown in `encoding`
    #[clap(long, group = "private_key_to_roate_to")]
    pub(crate) new_private_key: Option<String>,

    /// Name of the profile to save the new private key
    #[clap(long)]
    pub(crate) save_to_profile: Option<String>,
}

impl ParsePrivateKey for RotateKey {}

impl RotateKey {
    /// Extract private key from CLI args
    pub fn extract_private_key(
        &self,
        encoding: EncodingType,
    ) -> CliTypedResult<Option<Ed25519PrivateKey>> {
        self.parse_private_key(
            encoding,
            self.new_private_key_file.clone(),
            self.new_private_key.clone(),
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RotateSummary {
    message: Option<String>,
    transaction: TransactionSummary,
}

#[async_trait]
impl CliCommand<RotateSummary> for RotateKey {
    fn command_name(&self) -> &'static str {
        "RotateKey"
    }

    async fn execute(self) -> CliTypedResult<RotateSummary> {
        let new_private_key = self
            .extract_private_key(self.txn_options.encoding_options.encoding)?
            .ok_or_else(|| {
                CliError::CommandArgumentError(
                    "One of ['--new-private-key', '--new-private-key-file'] must be used"
                        .to_string(),
                )
            })?;

        let sender_address = self.txn_options.sender_address()?;

        // Get sequence number for account
        let sequence_number = self.txn_options.sequence_number(sender_address).await?;
        let auth_key = self.txn_options.auth_key(sender_address).await?;

        let rotation_proof = RotationProofChallenge {
            account_address: CORE_CODE_ADDRESS,
            module_name: "account".to_string(),
            struct_name: "RotationProofChallenge".to_string(),
            sequence_number,
            originator: sender_address,
            current_auth_key: AccountAddress::from_bytes(&auth_key)
                .map_err(|err| CliError::UnableToParse("auth_key", err.to_string()))?,
            new_public_key: new_private_key.public_key().to_bytes().to_vec(),
        };

        let rotation_msg =
            bcs::to_bytes(&rotation_proof).map_err(|err| CliError::BCS("rotation_proof", err))?;

        // Signs the struct using both the current private key and the next private key
        let rotation_proof_signed_by_current_private_key = self
            .txn_options
            .private_key()?
            .sign_arbitrary_message(&rotation_msg.clone());
        let rotation_proof_signed_by_new_private_key =
            new_private_key.sign_arbitrary_message(&rotation_msg);

        let txn_summary = self
            .txn_options
            .submit_transaction(
                aptos_stdlib::account_rotate_authentication_key(
                    0,
                    // Existing public key
                    self.txn_options
                        .private_key()?
                        .public_key()
                        .to_bytes()
                        .to_vec(),
                    0,
                    // New public key
                    new_private_key.public_key().to_bytes().to_vec(),
                    rotation_proof_signed_by_current_private_key
                        .to_bytes()
                        .to_vec(),
                    rotation_proof_signed_by_new_private_key.to_bytes().to_vec(),
                ),
                None,
            )
            .await
            .map(TransactionSummary::from)?;

        let string = serde_json::to_string_pretty(&txn_summary)
            .map_err(|err| CliError::UnableToParse("transaction summary", err.to_string()))?;

        eprintln!("{}", string);

        if let Some(txn_success) = txn_summary.success {
            if !txn_success {
                return Err(CliError::ApiError(
                    "Transaction was not executed successfully".to_string(),
                ));
            }
        } else {
            return Err(CliError::UnexpectedError(
                "Malformed transaction response".to_string(),
            ));
        }

        let mut profile_name: String;

        if self.save_to_profile.is_none() {
            if let Err(cli_err) = prompt_yes_with_override(
                "Do you want to create a profile for the new key?",
                self.txn_options.prompt_options,
            ) {
                match cli_err {
                    CliError::AbortedError => {
                        return Ok(RotateSummary {
                            transaction: txn_summary,
                            message: None,
                        });
                    }
                    _ => {
                        return Err(cli_err);
                    }
                }
            }

            eprintln!("Enter the name for the profile");
            profile_name = read_line("Profile name")?.trim().to_string();
        } else {
            // We can safely unwrap here
            profile_name = self.save_to_profile.unwrap();
        }

        // Check if profile name exists
        let mut config = CliConfig::load(ConfigSearchMode::CurrentDirAndParents)?;

        if let Some(ref profiles) = config.profiles {
            if profiles.contains_key(&profile_name) {
                if let Err(cli_err) = prompt_yes_with_override(
                    format!(
                        "Profile {} exits. Do you want to provide a new profile name?",
                        profile_name
                    )
                    .as_str(),
                    self.txn_options.prompt_options,
                ) {
                    match cli_err {
                        CliError::AbortedError => {
                            return Ok(RotateSummary {
                                transaction: txn_summary,
                                message: None,
                            });
                        }
                        _ => {
                            return Err(cli_err);
                        }
                    }
                }

                eprintln!("Enter the name for the profile");
                profile_name = read_line("Profile name")?.trim().to_string();
            }
        }

        if profile_name.is_empty() {
            return Err(CliError::AbortedError);
        }

        let mut profile_config = ProfileConfig {
            private_key: Some(new_private_key.clone()),
            public_key: Some(new_private_key.public_key()),
            account: Some(sender_address),
            ..self.txn_options.profile_options.profile()?
        };

        if let Some(url) = self.txn_options.rest_options.url {
            profile_config.rest_url = Some(url.into());
        }

        if config.profiles.is_none() {
            config.profiles = Some(BTreeMap::new());
        }

        config
            .profiles
            .as_mut()
            .unwrap()
            .insert(profile_name.clone(), profile_config);
        config.save()?;

        eprintln!("Profile {} is saved.", profile_name);

        Ok(RotateSummary {
            transaction: txn_summary,
            message: Some(format!("Profile {} is saved.", profile_name)),
        })
    }
}

/// Command to lookup the account adress through on-chain lookup table
///
#[derive(Debug, Parser)]
pub struct LookupAddress {
    #[clap(flatten)]
    pub(crate) encoding_options: EncodingOptions,

    #[clap(flatten)]
    pub(crate) public_key_options: PublicKeyInputOptions,

    #[clap(flatten)]
    pub(crate) profile_options: ProfileOptions,

    #[clap(flatten)]
    pub(crate) rest_options: RestOptions,
}

impl LookupAddress {
    pub(crate) fn public_key(&self) -> CliTypedResult<Ed25519PublicKey> {
        self.public_key_options.extract_public_key(
            self.encoding_options.encoding,
            &self.profile_options.profile,
        )
    }

    /// Builds a rest client
    fn rest_client(&self) -> CliTypedResult<Client> {
        self.rest_options.client(&self.profile_options.profile)
    }
}

#[async_trait]
impl CliCommand<AccountAddress> for LookupAddress {
    fn command_name(&self) -> &'static str {
        "LookupAddress"
    }

    async fn execute(self) -> CliTypedResult<AccountAddress> {
        let originating_resource = self
            .rest_client()?
            .get_account_resource(CORE_CODE_ADDRESS, "0x1::account::OriginatingAddress")
            .await
            .map_err(|err| CliError::ApiError(err.to_string()))?
            .into_inner()
            .ok_or_else(|| CliError::UnexpectedError("Unable to parse API response.".to_string()))?
            .data;

        let table_handle = originating_resource["address_map"]["handle"]
            .as_str()
            .ok_or_else(|| {
                CliError::UnexpectedError("Unable to parse table handle.".to_string())
            })?;

        // The derived address that can be used to look up the original address
        let address_key = AuthenticationKey::ed25519(&self.public_key()?).derived_address();

        Ok(AccountAddress::from_hex_literal(
            self.rest_client()?
                .get_table_item(
                    AccountAddress::from_str(table_handle)
                        .map_err(|err| CliError::UnableToParse("table_handle", err.to_string()))?,
                    "address",
                    "address",
                    address_key.to_hex_literal(),
                )
                .await
                .map_err(|err| CliError::ApiError(err.to_string()))?
                .into_inner()
                .as_str()
                .ok_or_else(|| {
                    CliError::UnexpectedError("Unable to parse API response.".to_string())
                })?,
        )
        .map_err(|err| CliError::UnableToParse("AccountAddress", err.to_string()))?)
    }
}

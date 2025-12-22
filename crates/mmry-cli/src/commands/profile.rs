use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::profile_blocks::ProfileBlockPatchOp;
use mmry_core::profile_blocks::ProfileBlockScope;
use mmry_core::profile_blocks::ProfileBlockWriteContext;
use mmry_core::profile_blocks::ProfileBlocksIngestOptions;
use mmry_core::profile_blocks::ProfileBlocksService;
use std::io::Read;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(about = "Manage user profile blocks (persona/human/etc)")]
pub struct ProfileCmd {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Subcommand)]
enum ProfileCommand {
    Blocks(ProfileBlocksCmd),
}

#[derive(Parser)]
pub struct ProfileBlocksCmd {
    #[command(subcommand)]
    command: ProfileBlocksCommand,
}

#[derive(Subcommand)]
enum ProfileBlocksCommand {
    /// List profile blocks for a user
    List(ProfileBlocksListCmd),
    /// Get a single profile block
    Get(ProfileBlocksGetCmd),
    /// Set (replace) a profile block
    Set(ProfileBlocksSetCmd),
    /// Apply patch operations to a profile block
    Patch(ProfileBlocksPatchCmd),
    /// Ingest a directory into profile blocks
    Ingest(ProfileBlocksIngestCmd),
}

#[derive(Parser)]
pub struct ProfileBlocksListCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Block scope (global, project, agent)")]
    pub scope: Option<String>,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProfileBlocksGetCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Block name (e.g. persona, human)")]
    pub block: String,

    #[arg(
        long,
        help = "Block scope (global, project, agent)",
        default_value = "project"
    )]
    pub scope: String,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProfileBlocksSetCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Actor id for audit events (defaults to user_id)")]
    pub actor_id: Option<String>,

    #[arg(long, help = "Block name (e.g. persona, human)")]
    pub block: String,

    #[arg(
        long,
        help = "Block scope (global, project, agent)",
        default_value = "project"
    )]
    pub scope: String,

    #[arg(long, help = "Block content or '-' to read from stdin")]
    pub content: String,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProfileBlocksPatchCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Actor id for audit events (defaults to user_id)")]
    pub actor_id: Option<String>,

    #[arg(long, help = "Block name (e.g. persona, human)")]
    pub block: String,

    #[arg(
        long,
        help = "Block scope (global, project, agent)",
        default_value = "project"
    )]
    pub scope: String,

    #[arg(
        long,
        help = "Patch operations as JSON array, or '-' to read from stdin"
    )]
    pub ops_json: String,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProfileBlocksIngestCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Actor id for audit events (defaults to user_id)")]
    pub actor_id: Option<String>,

    #[arg(
        long,
        help = "Block scope (global, project, agent)",
        default_value = "project"
    )]
    pub scope: String,

    #[arg(
        long,
        default_value = "ingest",
        help = "Prefix for generated block names"
    )]
    pub prefix: String,

    #[arg(long, default_value_t = 200, help = "Maximum files to scan")]
    pub max_files: usize,

    #[arg(long, default_value_t = 65536, help = "Maximum bytes to read per file")]
    pub max_file_bytes: usize,

    #[arg(long, default_value_t = 16, help = "Maximum number of blocks to write")]
    pub max_blocks: usize,

    #[arg(
        long,
        default_value = "md,txt,rs,toml,json,yaml,yml",
        help = "File extensions to include (comma-separated)"
    )]
    pub extensions: String,

    #[arg(long, help = "Include hidden files/directories")]
    pub include_hidden: bool,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,

    #[arg(long, help = "Dry run (do not write blocks)")]
    pub dry_run: bool,

    #[arg(help = "Directory to ingest")]
    pub path: PathBuf,
}

pub async fn handle(cmd: ProfileCmd, config: &Config, db: &Database) -> Result<()> {
    let svc = ProfileBlocksService::from_config(config);

    match cmd.command {
        ProfileCommand::Blocks(blocks) => match blocks.command {
            ProfileBlocksCommand::List(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let mut blocks = svc.list_blocks(db.pool(), user_id).await?;
                if let Some(scope) = args.scope.as_deref() {
                    let scope = scope.parse::<ProfileBlockScope>()?;
                    blocks.retain(|b| b.scope == scope);
                }
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&blocks)?);
                } else if blocks.is_empty() {
                    println!("(no blocks)");
                } else {
                    for block in blocks {
                        println!("{} {}", block.scope.as_str(), block.name);
                    }
                }
            }
            ProfileBlocksCommand::Get(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let scope = args.scope.parse::<ProfileBlockScope>()?;
                let block = svc
                    .get_block(db.pool(), user_id, &args.block, scope)
                    .await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&block)?);
                } else if let Some(block) = block {
                    print!("{}", block.content);
                    if !block.content.ends_with('\n') {
                        println!();
                    }
                } else {
                    anyhow::bail!("Block not found");
                }
            }
            ProfileBlocksCommand::Set(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let actor_id = args
                    .actor_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()?
                    .unwrap_or(user_id);

                let content = if args.content == "-" {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                } else {
                    args.content
                };

                let scope = args.scope.parse::<ProfileBlockScope>()?;
                let block = svc
                    .set_block(
                        db.pool(),
                        user_id,
                        &args.block,
                        content,
                        ProfileBlockWriteContext {
                            scope,
                            actor_id,
                            span_id: None,
                        },
                    )
                    .await?;

                if args.json {
                    println!("{}", serde_json::to_string_pretty(&block)?);
                } else {
                    println!("Updated {}", block.name);
                }
            }
            ProfileBlocksCommand::Patch(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let actor_id = args
                    .actor_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()?
                    .unwrap_or(user_id);

                let ops_raw = if args.ops_json == "-" {
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                } else {
                    args.ops_json
                };

                let ops: Vec<ProfileBlockPatchOp> = serde_json::from_str(&ops_raw)?;
                let scope = args.scope.parse::<ProfileBlockScope>()?;
                let block = svc
                    .patch_block(
                        db.pool(),
                        user_id,
                        &args.block,
                        ops,
                        ProfileBlockWriteContext {
                            scope,
                            actor_id,
                            span_id: None,
                        },
                    )
                    .await?;

                if args.json {
                    println!("{}", serde_json::to_string_pretty(&block)?);
                } else {
                    println!("Updated {}", block.name);
                }
            }
            ProfileBlocksCommand::Ingest(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let actor_id = args
                    .actor_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()?
                    .unwrap_or(user_id);

                let scope = args.scope.parse::<ProfileBlockScope>()?;
                let extensions = args
                    .extensions
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();

                let result = svc
                    .ingest_directory(
                        db.pool(),
                        user_id,
                        &args.path,
                        ProfileBlocksIngestOptions {
                            scope,
                            prefix: args.prefix,
                            skip_hidden: !args.include_hidden,
                            max_files: args.max_files,
                            max_file_bytes: args.max_file_bytes,
                            max_blocks: args.max_blocks,
                            extensions,
                            dry_run: args.dry_run,
                        },
                        actor_id,
                        None,
                    )
                    .await?;

                if args.json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Ingested {} file(s) into {} block(s) (written: {})",
                        result.files_included,
                        result.blocks.len(),
                        result.blocks.iter().filter(|b| b.written).count()
                    );
                    for block in result.blocks {
                        let suffix = if block.written { "" } else { " (unchanged)" };
                        println!("{} {}{}", block.scope.as_str(), block.name, suffix);
                    }
                }
            }
        },
    }

    Ok(())
}

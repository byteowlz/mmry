use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::profile_blocks::ProfileBlockPatchOp;
use mmry_core::profile_blocks::ProfileBlocksService;
use std::io::Read;
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
}

#[derive(Parser)]
pub struct ProfileBlocksListCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProfileBlocksGetCmd {
    #[arg(long, help = "Profile owner/user id (UUID)")]
    pub user_id: String,

    #[arg(long, help = "Block name (e.g. persona, human)")]
    pub block: String,

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
        help = "Patch operations as JSON array, or '-' to read from stdin"
    )]
    pub ops_json: String,

    #[arg(long, short = 'j', help = "Output as JSON")]
    pub json: bool,
}

pub async fn handle(cmd: ProfileCmd, config: &Config, db: &Database) -> Result<()> {
    let svc = ProfileBlocksService::from_config(config);

    match cmd.command {
        ProfileCommand::Blocks(blocks) => match blocks.command {
            ProfileBlocksCommand::List(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let blocks = svc.list_blocks(db.pool(), user_id).await?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&blocks)?);
                } else if blocks.is_empty() {
                    println!("(no blocks)");
                } else {
                    for block in blocks {
                        println!("{}", block.name);
                    }
                }
            }
            ProfileBlocksCommand::Get(args) => {
                let user_id = Uuid::parse_str(&args.user_id)?;
                let block = svc.get_block(db.pool(), user_id, &args.block).await?;
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

                let block = svc
                    .set_block(db.pool(), user_id, &args.block, content, actor_id, None)
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
                let block = svc
                    .patch_block(db.pool(), user_id, &args.block, ops, actor_id, None)
                    .await?;

                if args.json {
                    println!("{}", serde_json::to_string_pretty(&block)?);
                } else {
                    println!("Updated {}", block.name);
                }
            }
        },
    }

    Ok(())
}

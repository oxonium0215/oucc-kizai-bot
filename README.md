# OUCC Equipment Lending Bot

A comprehensive Discord bot for managing equipment reservations and lending in the OUCC (Osaka University Computer Club). Built with Rust using the Serenity Discord library.

## Features

- **Setup Command**: `/setup` to configure the bot in any channel
- **Reservation Management**: Visual reservation system with interactive embeds
- **Time Zone Support**: All times displayed in JST (UTC+9)
- **Equipment Organization**: Tag-based equipment categorization
- **Notification System**: DM-first notifications with channel fallback
- **Admin Management**: Comprehensive equipment and user management
- **Background Jobs**: Automated reminders and notifications
- **Self-Healing**: Automatic message synchronization on restart

## Setup Instructions

### Prerequisites

1. **Discord Bot Application**
   - Go to [Discord Developer Portal](https://discord.com/developers/applications)
   - Create a new application and bot
   - Copy the bot token
   - Enable the following bot permissions:
     - Read Messages/View Channels
     - Send Messages
     - Manage Messages
     - Embed Links
     - Read Message History
     - Use Slash Commands

2. **Required Intents**
   - Message Content Intent
   - Server Members Intent (optional, for better permission checking)

### Environment Variables

Create a `.env` file (for local development) or set these environment variables:

```bash
# Required
DISCORD_BOT_TOKEN=your_bot_token_here

# Optional
DATABASE_URL=sqlite:./bot.db  # Default: sqlite:./bot.db
LOG_LEVEL=info               # Default: info
```

### Running Locally

1. **Install Rust** (if not already installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

2. **Clone and build**:
   ```bash
   git clone https://github.com/oxonium0215/oucc-kizai-bot.git
   cd oucc-kizai-bot
   cargo build --release
   ```

3. **Run migrations**:
   ```bash
   # Install sqlx-cli if not already installed
   cargo install sqlx-cli --no-default-features --features sqlite
   
   # Run migrations
   sqlx migrate run
   ```

4. **Start the bot**:
   ```bash
   # Set environment variables
   export DISCORD_BOT_TOKEN="your_token_here"
   
   # Run the bot
   ./target/release/oucc-kizai-bot
   ```

### Running with Docker

1. **Build the image**:
   ```bash
   docker build -t oucc-kizai-bot .
   ```

2. **Run the container**:
   ```bash
   docker run -d \
     --name oucc-kizai-bot \
     -e DISCORD_BOT_TOKEN="your_token_here" \
     -v $(pwd)/data:/data \
     oucc-kizai-bot
   ```

3. **Using Docker Compose** (recommended):
   ```yaml
   version: '3.8'
   services:
     bot:
       build: .
       environment:
         - DISCORD_BOT_TOKEN=your_token_here
         - LOG_LEVEL=info
       volumes:
         - ./data:/data
       restart: unless-stopped
   ```

## Usage

### Initial Setup

1. **Invite the bot** to your Discord server with the required permissions
2. **Run `/setup`** in the channel you want to use for equipment management
3. **Confirm the setup** and optionally configure admin roles
4. **Use "Overall Management"** button to add your first equipment

### Equipment Management

- **Add Equipment**: Use Overall Management → Add Equipment
- **Configure Tags**: Organize equipment with custom tags
- **Set Locations**: Define lending and return locations
- **Manage Reservations**: Users can create, modify, and cancel reservations

### User Operations

- **New Reservation**: Click equipment embed buttons to create reservations
- **Return Equipment**: Mark equipment as returned with location
- **Transfer Ownership**: Transfer reservations to other users
- **Check Status**: View all your current reservations

## Development

### Project Structure

```
src/
├── main.rs           # Application entry point
├── config.rs         # Environment configuration
├── database.rs       # Database connection setup
├── handlers.rs       # Discord event handlers
├── commands.rs       # Slash commands implementation
├── jobs.rs          # Background job worker
├── models.rs        # Database models and types
├── time.rs          # JST time handling utilities
└── utils.rs         # Helper functions

migrations/           # Database migrations
.github/workflows/    # CI/CD pipeline
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

### Code Quality

```bash
# Format code
cargo fmt

# Run clippy linter
cargo clippy

# Check all at once
cargo fmt && cargo clippy && cargo test
```

## Database Schema

The bot uses SQLite with the following main tables:

- `guilds` - Server configuration
- `equipment` - Equipment items
- `reservations` - Reservation records
- `tags` - Equipment categorization
- `locations` - Lending/return locations
- `equipment_logs` - Audit trail
- `jobs` - Background job queue
- `managed_messages` - Discord message tracking

## Backup and Maintenance

### Database Backup

```bash
# Backup SQLite database
cp bot.db bot_backup_$(date +%Y%m%d_%H%M%S).db

# Or using SQLite tools
sqlite3 bot.db ".backup backup.db"
```

### Message Synchronization

The bot automatically synchronizes its managed messages on startup. If messages get out of sync:

1. Restart the bot - it will detect and fix inconsistencies
2. Use the "Overall Management" interface to refresh equipment displays
3. Check logs for any permission or API rate limit issues

### Troubleshooting

**Bot not responding to slash commands:**
- Verify the bot has "Use Slash Commands" permission
- Check if commands are registered (check logs on startup)
- Ensure the bot is online and has network connectivity

**Permission errors:**
- Verify the bot has required permissions in the target channel
- Check if the bot's role is above other roles it needs to manage
- Ensure Discord intents are properly configured

**Database issues:**
- Check file permissions for SQLite database
- Verify DATABASE_URL environment variable
- Run migrations if database is outdated: `sqlx migrate run`

**Rate limiting:**
- The bot implements backoff strategies
- If hitting rate limits frequently, check for permission loops
- Consider increasing delays in message update operations

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and linting
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Support

For issues and questions:
- Create an issue on GitHub
- Check the troubleshooting section above
- Review Discord bot permissions and setup
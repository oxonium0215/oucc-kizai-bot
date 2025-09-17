# OUCC Equipment Lending Bot

A comprehensive Discord bot for managing equipment reservations and lending in the OUCC (Osaka University Computer Club). Built with Rust using the Serenity Discord library.

## Features

- **Setup Command**: `/setup` to configure the bot in any channel
- **Interactive Reservations**: Visual reservation system with modal forms and real-time conflict detection
- **Time Zone Support**: All times displayed in JST (UTC+9) with automatic UTC conversion for storage
- **Equipment Organization**: Tag-based equipment categorization with custom sort orders
- **Permission Management**: User-level reservation management with admin override capabilities
- **Audit Logging**: Complete equipment operation history in equipment_logs table
- **Live Embed Updates**: Equipment availability refreshes automatically after reservation changes
- **Background Jobs**: Automated reminders and notifications (future features)
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
DATABASE_URL=sqlite://./data/bot.db  # Default: sqlite://./data/bot.db (note the data directory)
LOG_LEVEL=info                       # Default: info
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

3. **Prepare data directory and run migrations**:
   ```bash
   # Create data directory for SQLite database
   mkdir -p data
   
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
- **Configure Tags**: Organize equipment with custom tags (use sort order numbers for grouping)
- **Set Locations**: Define lending and return locations
- **Refresh Display**: Update equipment embeds after making changes
- **Manage Reservations**: Users can create, modify, and cancel reservations

**Note**: Only users with administrator permissions or configured admin roles can access Overall Management features.

### User Operations

#### Making Reservations

1. **Reserve Equipment**: Click the "📅 Reserve" button on any available equipment embed
   - Fill in start time in JST format: `YYYY-MM-DD HH:MM` (e.g., `2024-01-15 14:00`)
   - Fill in end time in JST format: `YYYY-MM-DD HH:MM` (e.g., `2024-01-15 16:00`)
   - Optionally specify return location (defaults to equipment's default location)
   - Maximum reservation length: 60 days

2. **Edit Reservations**: Click the "✏️ Edit" button on your active reservations
   - Modify start/end times or return location
   - Changes are subject to conflict detection with other reservations

3. **Cancel Reservations**: Click the "❌ Cancel" button on your reservations
   - Cancellations are immediate and free up the equipment for others

#### Time Format & Validation

- **Input Format**: `YYYY-MM-DD HH:MM` (24-hour format in JST)
- **Examples**: 
  - `2024-01-15 09:00` (9:00 AM on January 15, 2024)
  - `2024-12-25 13:30` (1:30 PM on December 25, 2024)
- **Restrictions**:
  - Start time must be in the future
  - End time must be after start time
  - Maximum 60 days from current time
  - Cannot overlap with existing confirmed reservations

#### Conflict Detection

The bot automatically prevents overlapping reservations:
- Real-time conflict checking when creating/editing reservations
- Displays conflicting reservation details if overlap detected
- Database-level transactions ensure atomic conflict resolution

#### Admin Features

Administrators can:
- Cancel any user's reservation using admin-only cancel buttons
- Access Overall Management panel for equipment/location/tag management
- View detailed equipment logs with full reservation history

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

## Testing

This project includes a comprehensive automated test suite that validates core domain logic and end-to-end workflows using mocked Discord interactions.

### Test Categories

**1. Unit Tests**
- Time conversion (UTC↔JST) with DST edge cases
- Reservation overlap detection and conflict resolution  
- Return correction window validation
- Transfer state machine transitions
- Equipment ordering (tag.sort_order + name)

**2. Integration Tests**
- Concurrent reservation attempts with atomic conflict detection
- Transfer timeout jobs with deterministic time advancement
- Return flow with location confirmation and correction windows
- Notification reminders (pre-end and return delay)
- DM failure fallback testing

**3. End-to-End Tests**
- Complete equipment lending lifecycle simulation
- Setup → add tags/locations/equipment → reservation → transfer → return
- Message self-healing on restart simulation
- JST formatting validation in all user-facing notifications

### Running the Test Suite

```bash
# Install SQLx CLI (if not already installed)
cargo install sqlx-cli --no-default-features --features sqlite

# Prepare test database and query cache
export DATABASE_URL=sqlite:test.db
touch test.db
sqlx migrate run
cargo sqlx prepare

# Run all tests
cargo test

# Run specific test categories
cargo test --test time_tests      # Time conversion tests
cargo test --test domain_tests    # Domain logic tests
cargo test --test transfer_tests  # Transfer state machine tests  
cargo test --test job_tests       # Job processing tests
cargo test --test e2e_happy_path  # End-to-end workflow tests

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_utc_to_jst_conversion
```

### Test Features

- **Deterministic Time**: `TestClock` allows precise time control for testing time-dependent logic
- **Mock Discord API**: `MockDiscordApi` captures all Discord interactions without requiring live Discord
- **Isolated Database**: Each test uses a fresh SQLite database with full schema migrations
- **Concurrent Testing**: Validates proper handling of race conditions and database transactions
- **Japanese Localization**: Ensures all user-facing times are displayed in JST format

### Continuous Integration

The CI pipeline automatically:
- Runs the full test suite on every commit
- Validates SQLx migrations and compile-time query checking
- Performs code formatting and linting checks
- Ensures offline compilation compatibility

Tests are designed to be fast, reliable, and maintainable without external dependencies.

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

## Testing & Validation

### End-to-End Validation
This project includes comprehensive validation checklists to ensure all features work correctly:

- **[End-to-End Validation Checklist](docs/e2e-validation-checklist.md)**: Complete validation procedures for all features
- **[Quick Reference](docs/e2e-quick-reference.md)**: Condensed checklist for rapid validation

Use these checklists:
- Before major releases
- After bug fixes 
- During feature development
- For periodic health checks

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Test specific module
cargo test test_name
```

### Code Quality

```bash
# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Build project
cargo build --release
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and linting (see Testing section above)
5. Update validation checklists if adding new features
6. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Support

For issues and questions:
- Create an issue on GitHub
- Check the troubleshooting section above
- Review Discord bot permissions and setup
# OUCC Equipment Lending Bot

A comprehensive Discord bot for managing equipment reservations and lending in the OUCC (Osaka University Computer Club). Built with Rust using the Serenity Discord library.

## Features

- **Setup Command**: `/setup` to configure the bot in any channel
- **Interactive Reservations**: Visual reservation system with modal forms and real-time conflict detection
- **Managed Reservation Channels**: Fully automated equipment display with user message auto-deletion
- **Minimal API Updates**: Intelligent message editing minimizes Discord API usage and preserves message history  
- **Time Zone Support**: All times displayed in JST (UTC+9) with automatic UTC conversion for storage
- **Equipment Organization**: Tag-based equipment categorization with custom sort orders
- **Permission Management**: User-level reservation management with admin override capabilities
- **Audit Logging**: Complete equipment operation history in equipment_logs table
- **Live Embed Updates**: Equipment availability refreshes automatically after reservation changes
- **Notification System**: Automated reminders via DM with channel fallback for reservation events
- **Self-Healing**: Automatic message synchronization and repair on restart

## Notifications & Reminders

The bot provides a comprehensive notification system that keeps users informed about their reservations while being respectful of Discord's rate limits and user preferences.

### Reminder Types

#### Pre-Start Reminder
- **When**: Configurable minutes before reservation starts (default: 15 minutes)
- **Purpose**: Reminds users their reservation is about to begin
- **Message**: Includes equipment name and start time in JST

#### Start Reminder  
- **When**: At the exact reservation start time
- **Purpose**: Notifies users their reservation has begun
- **Message**: Confirms reservation is now active

#### Pre-End Reminder
- **When**: Configurable minutes before reservation ends (default: 15 minutes)  
- **Purpose**: Reminds users to prepare for return
- **Message**: Includes equipment name and end time in JST

#### Overdue Reminders
- **When**: After reservation end time passes without return
- **Frequency**: Configurable intervals (default: every 12 hours)
- **Limit**: Configurable maximum count (default: 3 reminders)
- **Purpose**: Encourages timely equipment return

### Delivery Methods

#### Primary: Direct Messages (DM)
- **Preferred Method**: All reminders are sent as DMs first
- **Privacy Friendly**: Keeps reservation details private
- **User Control**: Users can disable DMs if preferred

#### Fallback: Channel Mentions
- **When DMs Fail**: If user has DMs disabled or bot lacks DM permissions
- **Configurable**: Can be enabled/disabled per guild in `/setup`
- **Non-Intrusive**: Short mentions with essential information only
- **Format**: `@user Equipment reminder: [brief message]`

#### Failed Delivery Handling
- **Graceful Degradation**: Records delivery attempt as "FAILED"
- **No Spam**: Will not retry failed deliveries automatically
- **Admin Visibility**: Failed deliveries can be tracked in database

### Configuration

#### During Setup (`/setup` command)
- **DM Fallback**: Enable/disable channel mentions when DMs fail
- **Pre-Start Timing**: 5, 15, or 30 minutes before start
- **Pre-End Timing**: 5, 15, or 30 minutes before end  
- **Overdue Frequency**: Every 6, 12, or 24 hours
- **Overdue Limit**: Maximum number of overdue reminders

#### Default Settings
```
DM Fallback: Enabled
Pre-Start: 15 minutes
Pre-End: 15 minutes  
Overdue: Every 12 hours (max 3 times)
```

### Message Examples

#### Pre-End Reminder (DM)
```
📅 リマインダー: 「Canon EOS R5」の貸出期限まで15分です。
返却時刻: 2024/01/15 17:00
```

#### Overdue Reminder (DM)
```
⚠️ 返却遅延 #2: 「Canon EOS R5」の返却期限が過ぎています。
期限: 2024/01/15 17:00
```

#### Channel Fallback (when DM fails)
```
<@user123> 📅 リマインダー: 「Canon EOS R5」の貸出期限まで15分です。
返却時刻: 2024/01/15 17:00
```

### Technical Implementation

#### Idempotency
- **Duplicate Prevention**: Each reminder type is sent only once per reservation
- **Database Tracking**: `sent_reminders` table prevents duplicates
- **Safe Retries**: Job system can safely retry without spam

#### Time Handling
- **UTC Storage**: All times stored in UTC for consistency
- **JST Display**: User-facing messages show JST (UTC+9)
- **Clock Jump Safe**: Handles system clock changes gracefully

#### Performance
- **Lightweight Scheduler**: Checks for due reminders every minute
- **Efficient Queries**: Optimized database queries with proper indexing
- **Rate Limit Aware**: Respects Discord's API rate limits

### Troubleshooting

#### "I'm not receiving reminders"
1. **Check DM Settings**: Ensure you haven't disabled DMs from server members
2. **Check Channel Fallback**: Look for mentions in the reservation channel
3. **Verify Timing**: Reminders are only sent for future reservations
4. **Admin Check**: Ask admin if DM fallback is enabled for the server

#### "Reminders sent to wrong time"
- **Time Zone**: All times are displayed in JST (UTC+9)
- **Configuration**: Check if reminder timing was customized during setup
- **System Clock**: Server time affects reminder delivery timing

#### "Getting duplicate reminders"  
- **Should Not Happen**: Each reminder type is sent only once
- **Report Issue**: Contact admin if experiencing duplicates
- **Database Check**: Admin can verify `sent_reminders` table

#### "No overdue reminders for returned items"
- **By Design**: Reminders automatically stop when items are marked as returned
- **Return Process**: Ensure equipment was properly returned through the bot
- **Status Check**: Admin can verify reservation return status

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
3. **Permission Check**: The bot will verify it has required permissions (Send Messages, Manage Messages, Embed Links, etc.)
4. **Role Selection**: Optionally select custom admin roles who can manage equipment (Step 1)
5. **Confirmation**: Review your settings and complete setup (Step 2)
6. **Use "Overall Management"** button to add your first equipment

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

### Overall Management

The Overall Management panel provides a comprehensive dashboard for equipment administrators to monitor and manage all reservations. Access it by clicking the "⚙️ Overall Management" button in the header message.

#### Access Control
- **Admin-only access**: Only guild administrators or users with configured admin roles can access
- **Ephemeral interactions**: All management interactions are private and don't create channel noise
- **Permission enforcement**: Non-admin users receive a polite denial message

#### Dashboard Features

**Current Filters Display**
- Shows active equipment, time, and status filters
- Real-time filter summary for easy reference

**Reservation Listings**
- Paginated view of filtered reservations (10 per page)
- Compact format: `[Equipment] start_jst → end_jst, @user • status • location`
- Navigation: Previous/Next page controls when needed

**Filter Controls**
- **🔧 Equipment Filter**: Multi-select dropdown with "All Equipment" option (up to 25 equipment items)
- **📅 Time Filter**: Preset options (Today, Next 24h, Next 7 days, All Time)
- **📊 Status Filter**: Active, Upcoming, Returned Today, All statuses
- **🗑️ Clear All**: Reset all filters to defaults

#### Bulk Actions

**🔄 Refresh Display**
- Triggers reconciliation of the equipment channel
- Updates all equipment embeds with current status
- Provides success/failure feedback
- Useful after manual database changes or system issues

**📊 Export CSV**
- Generates CSV export based on current filters
- Columns: Reservation ID, Equipment, User ID, Start/End Times (JST & UTC), Status, Location, Return info
- Shows export preview with summary statistics
- Includes applied filter information for reference

**🔗 Jump to Equipment** *(Coming Soon)*
- Will provide direct links to specific equipment embeds
- Planned feature for quick navigation to equipment messages

#### Usage Examples

**Find Active Loans for Specific Equipment:**
1. Click "🔧 Equipment Filter" → Select target equipment
2. Click "📊 Status Filter" → Select "Active"
3. View filtered results showing current borrowers

**Export Today's Returns:**
1. Click "📅 Time Filter" → Select "Today"
2. Click "📊 Status Filter" → Select "Returned Today"
3. Click "📊 Export CSV" for detailed return report

**Monitor Upcoming Reservations:**
1. Click "📅 Time Filter" → Select "Next 24h"
2. Click "📊 Status Filter" → Select "Upcoming"
3. Review reservations starting soon

#### Technical Details

- **State Management**: Per-user session state with automatic cleanup
- **Real-time Updates**: Filters update display immediately
- **Performance**: In-memory filtering for responsive interaction
- **Rate Limiting**: Respects Discord API limits with proper debouncing
- **JST Time Display**: All times shown in Japan Standard Time for user convenience

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
cargo test --test time_tests        # Time conversion tests
cargo test --test domain_tests      # Domain logic tests
cargo test --test transfer_tests    # Transfer state machine tests  
cargo test --test job_tests         # Job processing tests
cargo test --test reminder_tests    # Notification system tests
cargo test --test e2e_happy_path    # End-to-end workflow tests

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

- `guilds` - Server configuration and notification preferences
- `equipment` - Equipment items
- `reservations` - Reservation records
- `tags` - Equipment categorization
- `locations` - Lending/return locations
- `equipment_logs` - Audit trail
- `jobs` - Background job queue
- `sent_reminders` - Notification delivery tracking
- `managed_messages` - Discord message tracking

## Backup and Maintenance

### Database Backup

```bash
# Backup SQLite database
cp bot.db bot_backup_$(date +%Y%m%d_%H%M%S).db

# Or using SQLite tools
sqlite3 bot.db ".backup backup.db"
```

### Message Synchronization and Channel Management

The bot maintains **fully managed reservation channels** with the following behavior:

**Managed-Only Channels:**
- User messages are automatically deleted to keep channels clean and organized
- Only bot-generated equipment embeds and management interfaces are preserved
- Users can interact through buttons and modals - no typing required in reservation channels

**Intelligent Message Updates:**
- The bot uses minimal editing to update existing messages rather than recreating them
- Only creates, edits, or deletes messages when structurally necessary
- Preserves message history and minimizes Discord API usage
- Maintains stable message ordering through database-tracked sort orders

**Self-Healing on Startup:**
The bot automatically synchronizes its managed messages on startup and detects:
- Missing header or equipment messages
- Messages with incorrect content or ordering  
- Orphaned messages not tracked in the database
- Duplicate or conflicting message states

If messages get out of sync:
1. Restart the bot - it will detect and fix inconsistencies automatically
2. Use the "🔄 Refresh Display" button in the Overall Management interface
3. Check logs for any permission or API rate limit issues

**Performance Optimizations:**
- Edit plan computation minimizes Discord API calls (typically 0-2 API calls per refresh)
- Database indexes optimize message lookup and sorting
- Bulk operations are avoided in favor of targeted updates

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

**Overall Management Issues:**

*"You need administrator permissions" error:*
- Verify you have Administrator permission in the guild
- Check if you're assigned to configured admin roles from `/setup`
- Contact a guild administrator to grant proper permissions

*"No equipment found" when trying to filter:*
- Use Overall Management → Add Equipment to create equipment first
- Verify equipment was added to the correct guild
- Check if equipment was accidentally deleted

*Filters showing no results:*
- Try clearing all filters with "🗑️ Clear All" button
- Check if time filters are too restrictive (e.g., "Today" when no reservations exist)
- Verify equipment IDs are correct in equipment filter

*CSV export shows "coming soon":*
- CSV download feature provides preview data for now
- Use the dashboard view for current reservation monitoring
- Full file download functionality planned for future release

*Dashboard not updating after changes:*
- Use "🔄 Refresh Display" to trigger equipment channel reconciliation
- Check Discord API rate limits in bot logs
- Verify database connectivity if issues persist

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
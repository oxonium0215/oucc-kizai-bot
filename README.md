# OUCC Equipment Lending Bot

A comprehensive Discord bot for managing equipment reservations and lending in the OUCC (Osaka University Computer Club). Built with Rust using the Serenity Discord library.

## Features

- **Setup Command**: `/setup` to configure the bot in any channel
- **Interactive Reservations**: Visual reservation system with modal forms and real-time conflict detection
- **Owner Transfer**: Transfer reservations between users with immediate and scheduled options
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

#### Owner Transfer

Transfer ownership of your reservations to other users with flexible timing options.

**Access Points:**
- **Equipment Embeds**: "🔄 Transfer" button (shown when you own active/upcoming reservations for that equipment)
- **Overall Management Panel**: "🔄 Transfer #N" buttons for each reservation (admin-only for others' reservations)

**Transfer Types:**

1. **Immediate Transfer**
   - Transfers ownership instantly upon confirmation
   - Target user becomes the new reservation owner immediately
   - Equipment embeds refresh automatically

2. **Scheduled Transfer**
   - Schedule the transfer for a specific future time
   - Enter execution time in JST format: `YYYY-MM-DD HH:MM`
   - Transfer executes automatically at the scheduled time
   - Can be cancelled before execution

**Transfer Process:**
1. Click any "🔄 Transfer" button on equipment with your reservations
2. Enter the Discord User ID of the new owner
3. Choose transfer type: `immediate` or `schedule`
4. For scheduled transfers: specify execution time in JST
5. Optional: Add a note explaining the transfer
6. Confirm to execute (immediate) or schedule the transfer

**Permissions:**
- **Reservation Owners**: Can transfer their own reservations
- **Administrators**: Can transfer any user's reservations
- **Target Users**: Must be guild members (not bots)
- **No-op Prevention**: Cannot transfer to the same user

**Validation & Constraints:**
- **Timing**: Scheduled transfers must be within `[max(now, reservation_start), reservation_end)`
- **Status**: Cannot transfer returned reservations
- **Conflicts**: Cannot create multiple pending transfers for the same reservation
- **Expiry**: Scheduled transfers are automatically cancelled if reservation ends first

**Cancellation:**
- **Who**: Original requester or administrators can cancel scheduled transfers
- **When**: Any time before execution
- **How**: Click "🚫 Cancel Transfer" button on the transfer confirmation message

**Examples:**

*Immediate handoff when leaving early:*
1. Click "🔄 Transfer" on your active reservation
2. Enter colleague's Discord User ID
3. Type: `immediate`
4. Note: `Leaving early, please take over`
5. Confirm → Transfer happens instantly

*Scheduled handoff for shift change:*
1. Click "🔄 Transfer" on your upcoming reservation  
2. Enter replacement user's Discord User ID
3. Type: `schedule`
4. Time: `2024-01-15 14:00` (shift change time)
5. Note: `Afternoon shift handover`
6. Confirm → Transfer scheduled for execution

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
- Create and manage maintenance windows for equipment

## Maintenance & Blackouts

The bot supports scheduled maintenance windows that make equipment temporarily unavailable for reservations. This is useful for cleaning, repairs, inspections, or other equipment downtime.

### Creating Maintenance Windows

**Access**: Admin-only feature accessible through equipment embeds

1. **New Maintenance**: Click "🔧 Maintenance" button on any equipment embed
2. **Fill Details**: Specify start time, end time, and optional reason in JST format
3. **Automatic Conflict Detection**: System prevents overlaps with existing reservations and maintenance

#### Time Format
- **Format**: `YYYY-MM-DD HH:MM` (24-hour format in JST)
- **Examples**: 
  - `2024-01-15 09:00` (9:00 AM on January 15, 2024)
  - `2024-12-25 13:30` (1:30 PM on December 25, 2024)

#### Validation Rules
- Start time must be in the future
- End time must be after start time
- Cannot overlap with existing reservations
- Cannot overlap with other maintenance windows on the same equipment

### Managing Existing Maintenance

When equipment has scheduled maintenance, the embed shows additional buttons:

- **🔧 Edit Maintenance**: Modify times or reason (with conflict checking)
- **❌ Cancel Maintenance**: Remove the maintenance window

### Conflict Prevention

The system enforces strict conflict prevention:

#### Reservation Conflicts
- **Creation**: New reservations are blocked during maintenance periods
- **Modification**: Existing reservations cannot be changed to overlap maintenance
- **Clear Messages**: Users see specific maintenance details when conflicts occur

Example conflict message:
```
❌ Reservation conflicts with scheduled maintenance (Equipment cleaning) 
from 2024-01-15 09:00 to 2024-01-15 17:00. Please choose a different time.
```

#### Maintenance Conflicts
- **Overlap Prevention**: Multiple maintenance windows cannot overlap on the same equipment
- **Edit Protection**: Maintenance edits are validated against existing windows
- **Visual Feedback**: Clear error messages when overlaps are attempted

### Display Integration

#### Equipment Embeds
Maintenance status is prominently displayed in equipment embeds:

**Current Maintenance**:
```
🔧 Current Maintenance
Equipment cleaning
Until: 2024-01-15 17:00
```

**Upcoming Maintenance**:
```
🔧 Scheduled Maintenance
Equipment cleaning
From: 2024-01-15 09:00 to 2024-01-15 17:00
```

#### Automatic Updates
- Equipment embeds update automatically when maintenance is created/edited/canceled
- Maintenance buttons change based on current status
- Real-time conflict detection during reservation flows

### Notifications (Planned)

When maintenance overlaps existing reservations, affected users receive notifications with:
- Equipment name and maintenance details
- Affected time periods in JST
- Quick action buttons to modify or cancel their reservations

### Troubleshooting

#### "Cannot create overlapping maintenance"
- **Check Schedule**: Verify no existing maintenance windows overlap
- **Time Boundaries**: Ensure start/end times don't conflict with other windows
- **Reload Interface**: Refresh equipment embed to see current maintenance status

#### "Maintenance conflicts with reservation"
- **Review Reservations**: Check existing bookings during the proposed time
- **Coordinate with Users**: Contact affected users to reschedule if needed
- **Force Override**: Cancel conflicting reservations first (if appropriate)

#### "Users not notified about maintenance"
- **DM Settings**: Users may have disabled direct messages
- **Channel Notifications**: Check if fallback channel notifications are enabled
- **Manual Communication**: Consider posting in channel for important maintenance

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
- **Quick Actions**: Transfer buttons for each reservation (🔄 Transfer #N)
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

## Waitlist & Auto-Offers

The bot includes a comprehensive waitlist system that automatically manages equipment availability and notifies users when their desired time slots become available.

### How It Works

#### Joining the Waitlist

When a user tries to reserve equipment but encounters a conflict:

1. **Automatic Detection**: The system detects reservation conflicts or holds
2. **Join Waitlist Option**: A "Join Waitlist" button appears automatically
3. **FIFO Queue**: Users are added to a fair first-in-first-out queue
4. **Time Window Tracking**: System remembers the exact desired time range

#### Automatic Offers

When equipment becomes available (due to cancellations, early returns, or maintenance completion):

1. **Next-in-Line Selection**: The system finds the next user in the FIFO queue
2. **Time Window Matching**: Only offers if the available window fully covers the user's desired time
3. **Exclusive Hold**: A temporary hold prevents others from booking during the offer period
4. **Instant Notification**: User receives a DM with Accept/Decline buttons

### User Experience

#### Waitlist Entry

```
✅ Successfully Joined Waitlist!

📋 Entry ID: 42
📅 Desired Time: 2024/01/15 14:00 to 16:00 (JST)

🔔 You'll be notified via DM when this time slot becomes available. 
Your position in the queue is based on when you joined the waitlist.
```

#### Offer Notification (DM)

```
🎉 Equipment Available!

Equipment: Professional Camera
Available Time: 2024/01/15 14:00 to 16:00 (JST)
Offer Expires: 2024/01/15 13:45 (JST)

You have been waitlisted for this equipment and it's now available! 
Click the buttons below to accept or decline this offer.

[Accept Offer] [Decline Offer]
```

#### Successful Acceptance

```
✅ Offer Accepted Successfully!

🆔 Reservation ID: 156

Your reservation has been created and you have been removed from the waitlist.
```

### Features

#### Fair Queuing
- **FIFO Ordering**: First-joined gets first offer
- **No Cutting**: Queue position cannot be changed (except by admin)
- **Automatic Advancement**: Queue progresses when offers are declined or expire

#### Smart Matching
- **Exact Coverage**: Only offers time windows that fully cover the requested period
- **Conflict Prevention**: Temporary holds prevent double-booking during offers
- **Expiration**: Offers automatically expire (default: 15 minutes)

#### User Management
- **View Entries**: Users can see all their active waitlist entries
- **Leave Anytime**: Cancel waitlist entries at any time
- **Multiple Entries**: Can waitlist for different equipment or time windows

#### Admin Controls
- **Cancel Entries**: Admins can cancel any waitlist entry
- **Manual Offers**: Admins can create offers out of order (planned)
- **Queue Visibility**: View all waitlist entries for equipment management

### Technical Details

#### Hold System
When an offer is created, the system places a temporary exclusive hold on the time window:
- **Duration**: Configurable per guild (default: 15 minutes)
- **Enforcement**: Prevents all other users from reserving the held time
- **Exception**: The user with the offer can still accept (bypasses their own hold)
- **Auto-Release**: Hold automatically releases when offer expires or is declined

#### Trigger Events
Waitlist processing is automatically triggered when:
- **Reservation Cancellation**: Cancelled reservations immediately trigger waitlist processing
- **Early Returns**: Equipment returned before scheduled end time
- **Maintenance Completion**: When maintenance windows end or are cancelled

#### Notification Delivery
- **Primary**: Direct messages with interactive Accept/Decline buttons
- **Fallback**: Channel mentions if DMs are disabled or fail
- **Logging**: All notification attempts are logged for troubleshooting

### Troubleshooting

#### "No offers despite cancellations"
- **Partial Availability**: The available window must fully cover your desired time
- **Queue Position**: Others may be ahead of you in the queue
- **Time Constraints**: The available window might be too small

#### "Offer expired before I could respond"
- **Default Timeout**: Offers expire after 15 minutes by default
- **Admin Setting**: Your server admin can adjust the timeout period
- **Queue Advancement**: When offers expire, the next person gets notified

#### "Not receiving offer notifications"
1. **Check DM Settings**: Ensure you accept DMs from server members
2. **Check Channel**: Look for mentions in the reservation channel
3. **Verify Entry**: Use waitlist view to confirm your entry exists
4. **Contact Admin**: Admin can check notification delivery logs

#### "Joined waitlist but still see conflicts"
- **By Design**: Waitlist doesn't remove the underlying conflict
- **Time Flexibility**: Consider adjusting your desired time window
- **Multiple Entries**: You can create separate entries for different time ranges

### Configuration

#### Guild Settings
Admins can configure waitlist behavior during setup:
- **Offer Hold Duration**: How long offers remain valid (default: 15 minutes)
- **DM Fallback**: Whether to use channel mentions when DMs fail

#### Database Tables
- **waitlist_entries**: User requests for equipment/time windows
- **waitlist_offers**: System-generated offers with expiration tracking
- **Constraints**: Prevents duplicate entries for same user/equipment/window

### Examples

#### Basic Workflow
1. User tries to reserve camera for 2-4 PM → Conflict detected
2. User clicks "Join Waitlist" → Entry created with ID #42
3. Another user cancels reservation for 2-4 PM → System detects availability
4. System creates offer for user #42 → DM sent with Accept/Decline buttons
5. User clicks "Accept Offer" → Reservation created, waitlist entry removed

#### Multiple Users
1. Users A, B, C all waitlist camera for 2-4 PM (in that order)
2. Camera becomes available for 2-4 PM
3. User A gets first offer → 15 minutes to respond
4. If User A declines → User B gets next offer automatically
5. If User B accepts → Reservation created, Users A & C remain waitlisted

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

**Transfer-related issues:**

*"Cannot transfer to a bot user":*
- Only human users (guild members) can receive transfers
- Verify the target User ID belongs to a real user, not a bot

*"Transfer request already pending":*
- Only one pending transfer is allowed per reservation
- Cancel the existing transfer or wait for it to be resolved/expired

*"Invalid time format for scheduled transfer":*
- Use exactly this format: `YYYY-MM-DD HH:MM` in JST
- Example: `2024-01-15 14:30` for January 15th, 2:30 PM JST
- Ensure time is within the reservation period

*"Cannot transfer to the same user":*
- No-op transfers (to the same owner) are prevented
- Check that you entered a different user's Discord ID

*"Scheduled transfer was cancelled":*
- Transfers are auto-cancelled if the reservation is returned or ends before execution
- Check if the reservation was cancelled or returned by the original owner

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
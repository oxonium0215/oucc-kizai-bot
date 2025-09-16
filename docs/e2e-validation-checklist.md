# End-to-End Feature Validation Checklist

This document provides a comprehensive checklist for validating all major features of the OUCC Equipment Lending Bot. Update this checklist as features are completed or bugs are found.

## How to Use This Checklist

- **Pre-deployment**: Run through all applicable sections before major releases
- **Bug fixes**: Re-validate the affected feature area after fixes
- **New features**: Add new validation steps to the appropriate section
- **Regular maintenance**: Use for periodic health checks

Each validation item includes:
- ✅ **Validation Steps**: What to test
- 📋 **Expected Behavior**: What should happen
- ⚠️ **Edge Cases**: Potential failure scenarios to test

---

## 1. Setup & Configuration

### 1.1 Initial Bot Setup

#### `/setup` Command Execution
- ✅ **Validation Steps**:
  - Run `/setup` command in a test channel
  - Verify admin permission check works
  - Test with non-admin user (should fail)
  - Test in different channel types (text, thread, etc.)

- 📋 **Expected Behavior**:
  - Only administrators can run the command
  - Shows confirmation dialog with channel mention
  - Warning about deleting existing messages is displayed
  - Setup completion message appears after confirmation

- ⚠️ **Edge Cases**:
  - Bot lacks permissions in target channel
  - Multiple simultaneous setup attempts
  - Setup in DM channels (should fail gracefully)
  - Interrupting setup process mid-way

#### Channel Configuration & Permissions
- ✅ **Validation Steps**:
  - Verify bot can read/write messages in reservation channel
  - Test auto-deletion of user messages in reservation channel
  - Confirm bot messages are preserved during cleanup
  - Check permission inheritance in thread channels

- 📋 **Expected Behavior**:
  - User messages deleted immediately in reservation channel
  - Bot messages (embeds, management UI) remain untouched
  - Error logging for permission failures
  - Graceful degradation when permissions are insufficient

- ⚠️ **Edge Cases**:
  - Bot role moved below other roles
  - Channel permissions changed after setup
  - Voice/stage channels (unsupported scenarios)

#### Admin Role Selection
- ✅ **Validation Steps**:
  - Configure custom admin roles via database
  - Test permission checks with custom admin roles
  - Verify role hierarchy considerations
  - Test role removal/deletion scenarios

- 📋 **Expected Behavior**:
  - Custom admin roles stored in JSON format in database
  - Role-based permissions work alongside Discord admin permissions
  - Graceful handling of deleted/invalid roles

- ⚠️ **Edge Cases**:
  - Malformed JSON in admin_roles field
  - Non-existent role IDs in configuration
  - Role permissions changing after configuration

### 1.2 Database & Migrations

#### Database Schema Validation
- ✅ **Validation Steps**:
  - Run `sqlx migrate run` on fresh database
  - Verify all tables created with correct schema
  - Check foreign key constraints are enforced
  - Test database file permissions and backup/restore

- 📋 **Expected Behavior**:
  - All tables from migration files created successfully
  - Indexes created for performance-critical queries
  - Foreign key cascading works correctly
  - Database handles concurrent connections properly

- ⚠️ **Edge Cases**:
  - Migration on corrupted database
  - Disk space exhaustion during migration
  - Concurrent migration attempts
  - Rollback scenarios (not currently supported)

---

## 2. Message Visualization & Management

### 2.1 Equipment Embeds

#### Message Ordering & Display
- ✅ **Validation Steps**:
  - Add multiple equipment items across different tags
  - Verify sorting by tag order, then by equipment name
  - Test embed appearance and formatting
  - Check embed updates when equipment status changes

- 📋 **Expected Behavior**:
  - Equipment grouped by tags in configured order
  - Within tags, equipment sorted alphabetically
  - Status indicators (Available/Loaned/Unavailable) clearly visible
  - Real-time updates reflect current availability

- ⚠️ **Edge Cases**:
  - Very long equipment names (embed limits)
  - Special characters in names and descriptions
  - Equipment without tags (should appear in default group)
  - More than 25 equipment items (Discord embed limits)

#### Auto-Delete & Message Management
- ✅ **Validation Steps**:
  - Post user messages in reservation channel
  - Verify immediate deletion of user content
  - Confirm bot messages remain untouched
  - Test message edit/update scenarios

- 📋 **Expected Behavior**:
  - User messages deleted within seconds
  - Bot embeds and management messages preserved
  - Edit history maintained for bot messages
  - Error logging when deletion fails

- ⚠️ **Edge Cases**:
  - Message deletion during Discord outages
  - Bulk message scenarios (mass spam)
  - Messages with attachments or rich content
  - Bot messages that fail to delete (permission issues)

### 2.2 Self-Healing & Synchronization

#### Message Rebuild on Restart
- ✅ **Validation Steps**:
  - Stop bot while equipment embeds exist
  - Restart bot and verify message synchronization
  - Test with orphaned messages (equipment deleted)
  - Verify managed_messages table accuracy

- 📋 **Expected Behavior**:
  - All equipment embeds rebuilt from database state
  - Orphaned Discord messages removed
  - managed_messages table updated correctly
  - No duplicate messages created

- ⚠️ **Edge Cases**:
  - Discord API rate limits during rebuild
  - Messages in channels bot can no longer access
  - Database/Discord state conflicts
  - Partial rebuild failures

---

## 3. Reservation & Lending System

### 3.1 Reservation Creation Wizard

#### Step-by-Step Flow
- ✅ **Validation Steps**:
  - Click equipment embed button to start reservation
  - Navigate through date/time selection wizard
  - Test timezone handling (JST conversion)
  - Complete reservation and verify database entry

- 📋 **Expected Behavior**:
  - Clear step-by-step guidance through process
  - Date/time inputs validated before proceeding
  - JST times displayed to users
  - Confirmation message with reservation details

- ⚠️ **Edge Cases**:
  - Invalid date selections (past dates, far future)
  - Rapid clicking/multiple wizard instances
  - Session timeouts during lengthy reservation process
  - Browser/client timezone differences

#### Conflict Detection
- ✅ **Validation Steps**:
  - Create overlapping reservation attempts
  - Test edge cases (same start/end times)
  - Verify conflict messaging is clear
  - Test multiple concurrent reservation attempts

- 📋 **Expected Behavior**:
  - Clear conflict messages with specific details
  - Suggests alternative time slots when possible
  - Prevents double-booking completely
  - Graceful handling of race conditions

- ⚠️ **Edge Cases**:
  - Microsecond-level timing conflicts
  - Database transaction rollbacks
  - Conflicting reservations across midnight
  - Timezone-related conflict edge cases

### 3.2 Reservation Management

#### Change/Cancel/Transfer Flows
- ✅ **Validation Steps**:
  - Modify existing reservation times
  - Cancel active and future reservations
  - Transfer reservations to other users
  - Test permission boundaries (own reservations only)

- 📋 **Expected Behavior**:
  - Only reservation owners can modify their bookings
  - Clear confirmation dialogs for destructive actions
  - Equipment status updates reflect changes immediately
  - Audit trail maintained in equipment_logs

- ⚠️ **Edge Cases**:
  - Transferring to non-existent users
  - Canceling during active lending period
  - Rapid modification attempts
  - Transfer timeouts and expiration

#### Exclusive UI Access
- ✅ **Validation Steps**:
  - Test multiple users accessing same equipment simultaneously
  - Verify button states during active interactions
  - Check session isolation between users
  - Test interaction timeouts

- 📋 **Expected Behavior**:
  - Buttons disabled for other users during active session
  - Clear indicators of who is currently interacting
  - Automatic session cleanup after timeout
  - No interference between different user sessions

- ⚠️ **Edge Cases**:
  - Bot restart during active sessions
  - Very long user sessions (hours)
  - Network interruptions during interactions
  - Multiple devices for same user

---

## 4. Return Process

### 4.1 Location Selection

#### Return Location Management
- ✅ **Validation Steps**:
  - Configure multiple return locations
  - Test location selection during return process
  - Verify default location suggestions
  - Test custom location entry

- 📋 **Expected Behavior**:
  - Dropdown/selection of predefined locations
  - Default location based on equipment configuration
  - Option to enter custom location if needed
  - Location validation and normalization

- ⚠️ **Edge Cases**:
  - Very long location names (database limits)
  - Special characters in location names
  - Deleted locations with active references
  - Multiple locations with similar names

### 4.2 Return Confirmation

#### Time-Window Rules
- ✅ **Validation Steps**:
  - Test return before scheduled start time
  - Return during active lending period
  - Test late returns after end time
  - Verify grace period handling

- 📋 **Expected Behavior**:
  - Early returns allowed with confirmation
  - Normal returns processed immediately
  - Late returns flagged but still processed
  - Time-based validation with clear error messages

- ⚠️ **Edge Cases**:
  - Returns exactly at boundary times
  - Timezone-related return timing issues
  - System clock discrepancies
  - Return attempts for cancelled reservations

---

## 5. Notification System

### 5.1 DM-First Strategy

#### Direct Message Delivery
- ✅ **Validation Steps**:
  - Test DM delivery for various notification types
  - Verify DM failure detection and fallback
  - Test with users who block bot DMs
  - Check notification content and formatting

- 📋 **Expected Behavior**:
  - Primary attempt via direct messages
  - Rich formatting in DM notifications
  - Clear action buttons where applicable
  - Immediate fallback on DM failure

- ⚠️ **Edge Cases**:
  - Users with disabled DMs globally
  - Bot blocked by specific users
  - DM rate limiting scenarios
  - Very long notification content

### 5.2 Channel Fallback

#### Public Channel Notifications
- ✅ **Validation Steps**:
  - Trigger notifications when DMs fail
  - Verify channel mention notifications
  - Test notification privacy (no sensitive data in public)
  - Check channel permission requirements

- 📋 **Expected Behavior**:
  - Mentions user in reservation channel
  - Generic message with direction to check DMs
  - No sensitive reservation details in public
  - Clear instructions for next steps

- ⚠️ **Edge Cases**:
  - Channel deletion during fallback attempt
  - No permission to mention users in channel
  - Channel rate limiting
  - User notifications disabled entirely

### 5.3 Automated Reminders

#### Reminder Scheduling & Delivery
- ✅ **Validation Steps**:
  - Schedule reminders for upcoming reservations
  - Test different reminder intervals (1 day, 1 hour)
  - Verify reminder cancellation when reservation changes
  - Check reminder accuracy and timing

- 📋 **Expected Behavior**:
  - Reminders sent at appropriate intervals before reservation
  - Cancelled/modified reservations stop sending reminders
  - Clear reminder content with reservation details
  - Option to disable reminders per user

- ⚠️ **Edge Cases**:
  - System clock changes affecting reminder timing
  - Database cleanup of old reminder jobs
  - Rapid reservation changes causing reminder conflicts
  - Bot downtime during scheduled reminder time

### 5.4 Transfer Timeouts

#### Transfer Request Management
- ✅ **Validation Steps**:
  - Create transfer requests with various expiration times
  - Test automatic expiration handling
  - Verify notification sequences during transfer process
  - Check transfer acceptance/denial flows

- 📋 **Expected Behavior**:
  - Clear expiration times communicated to both parties
  - Automatic cleanup of expired transfers
  - Status updates sent to relevant users
  - No orphaned transfer requests

- ⚠️ **Edge Cases**:
  - Transfer requests expiring during system downtime
  - Rapid acceptance/denial of transfer requests
  - User account deletion during active transfers
  - Transfer of already-expired reservations

---

## 6. Administrative Management

### 6.1 Equipment Management UI

#### Equipment CRUD Operations
- ✅ **Validation Steps**:
  - Add, edit, and delete equipment through admin UI
  - Test bulk operations on multiple equipment
  - Verify equipment status transitions
  - Check equipment search and filtering

- 📋 **Expected Behavior**:
  - Intuitive admin interface for equipment management
  - Real-time updates to equipment embeds
  - Proper validation of equipment data
  - Confirmation dialogs for destructive operations

- ⚠️ **Edge Cases**:
  - Deleting equipment with active reservations
  - Editing equipment during active lending
  - Very large equipment databases (performance)
  - Concurrent admin modifications

### 6.2 Tag & Location Management

#### Organizational Tools
- ✅ **Validation Steps**:
  - Create, modify, and delete equipment tags
  - Reorder tags and verify embed update
  - Manage lending/return locations
  - Test tag assignment and removal

- 📋 **Expected Behavior**:
  - Drag-and-drop or numbered ordering for tags
  - Immediate visual updates in equipment displays
  - Location management with usage tracking
  - Bulk tag assignment capabilities

- ⚠️ **Edge Cases**:
  - Deleting tags with assigned equipment
  - Circular tag dependencies
  - Special characters in tag/location names
  - Performance with hundreds of tags/locations

### 6.3 User & Role Management

#### Permission Administration
- ✅ **Validation Steps**:
  - Configure custom admin roles
  - Test role-based access controls
  - Manage user permissions and restrictions
  - Verify audit logging for admin actions

- 📋 **Expected Behavior**:
  - Fine-grained permission controls
  - Clear role hierarchy and inheritance
  - Comprehensive audit trail for admin actions
  - User-friendly role assignment interface

- ⚠️ **Edge Cases**:
  - Role conflicts and permission overlaps
  - Revoking permissions from active admins
  - Role deletion with active assignments
  - Permission escalation attempts

### 6.4 Logging & Audit Trail

#### Equipment Logs & Analytics
- ✅ **Validation Steps**:
  - Generate various equipment log entries
  - Test log filtering and search capabilities
  - Verify log retention and archival
  - Check analytics and reporting features

- 📋 **Expected Behavior**:
  - Comprehensive logging of all equipment actions
  - Searchable log interface with filters
  - Export capabilities for reporting
  - Automatic log retention policies

- ⚠️ **Edge Cases**:
  - Very high activity periods (log volume)
  - Log corruption or data loss scenarios
  - Privacy considerations in logging
  - Long-term storage and performance

---

## 7. Background Job System

### 7.1 Job Queue Management

#### Job Processing & Reliability
- ✅ **Validation Steps**:
  - Schedule various job types (reminders, transfers)
  - Test job execution timing and accuracy
  - Verify job retry mechanisms
  - Check job failure handling and logging

- 📋 **Expected Behavior**:
  - Jobs execute at scheduled times
  - Failed jobs retry with exponential backoff
  - Job status tracking and monitoring
  - Dead letter queue for permanently failed jobs

- ⚠️ **Edge Cases**:
  - System clock changes affecting job scheduling
  - Database connection failures during job execution
  - Very high job volume (thousands queued)
  - Jobs scheduled far in the future

### 7.2 Retry Logic & Error Handling

#### Fault Tolerance
- ✅ **Validation Steps**:
  - Force job failures to test retry logic
  - Test maximum retry attempts
  - Verify exponential backoff timing
  - Check error reporting and alerting

- 📋 **Expected Behavior**:
  - Progressive retry delays (5min, 15min, 1hr)
  - Maximum retry limits prevent infinite loops
  - Clear error logging with stack traces
  - Alert mechanisms for critical job failures

- ⚠️ **Edge Cases**:
  - Transient vs permanent failure detection
  - Retry storms during system outages
  - Memory leaks in long-running job workers
  - Job corruption in retry scenarios

---

## 8. System Restart & Recovery

### 8.1 State Synchronization

#### Restart Recovery Process
- ✅ **Validation Steps**:
  - Restart bot with existing equipment and reservations
  - Verify message synchronization accuracy
  - Test job queue restoration
  - Check database consistency after restart

- 📋 **Expected Behavior**:
  - All Discord messages rebuilt from database state
  - Active reservations and jobs restored correctly
  - No duplicate messages or jobs created
  - Graceful handling of orphaned data

- ⚠️ **Edge Cases**:
  - Restart during active user interactions
  - Database corruption or connection failures
  - Discord API changes between restarts
  - Partial state restoration scenarios

### 8.2 Message Rebuild

#### Discord State Restoration
- ✅ **Validation Steps**:
  - Delete Discord messages while bot is offline
  - Restart and verify proper message recreation
  - Test with various message types and states
  - Check managed_messages table updates

- 📋 **Expected Behavior**:
  - Accurate recreation of all managed messages
  - Proper cleanup of orphaned Discord messages
  - Database synchronization with Discord state
  - Performance optimization for large message counts

- ⚠️ **Edge Cases**:
  - Rate limiting during bulk message recreation
  - Messages in inaccessible channels
  - Conflicting message IDs or duplicates
  - Memory usage during large rebuilds

### 8.3 No Duplicate Jobs

#### Job Deduplication
- ✅ **Validation Steps**:
  - Restart bot with pending jobs in queue
  - Verify no duplicate job creation
  - Test job uniqueness constraints
  - Check job scheduling consistency

- 📋 **Expected Behavior**:
  - Existing jobs continue from previous state
  - No duplicate jobs created on restart
  - Job timing preserved across restarts
  - Proper job status management

- ⚠️ **Edge Cases**:
  - Jobs scheduled exactly at restart time
  - Multiple bot instances (not supported)
  - Job queue corruption scenarios
  - Clock synchronization issues

---

## 9. Database Integrity & Performance

### 9.1 Migration Management

#### Schema Evolution
- ✅ **Validation Steps**:
  - Run migrations on fresh database
  - Test migration rollback scenarios (if supported)
  - Verify data preservation during migrations
  - Check migration performance on large datasets

- 📋 **Expected Behavior**:
  - Migrations run successfully in order
  - Existing data preserved during schema changes
  - Foreign key constraints maintained
  - Performance remains acceptable during migrations

- ⚠️ **Edge Cases**:
  - Migration failures mid-process
  - Large database migration performance
  - Concurrent access during migrations
  - Migration dependency conflicts

### 9.2 Data Integrity

#### Constraint Validation
- ✅ **Validation Steps**:
  - Test foreign key constraints
  - Verify unique constraints (equipment names, etc.)
  - Check data validation rules
  - Test cascade deletion scenarios

- 📋 **Expected Behavior**:
  - Database constraints prevent invalid data
  - Cascade deletions work correctly
  - Orphaned records cleaned up automatically
  - Transaction isolation maintained

- ⚠️ **Edge Cases**:
  - Constraint violations during bulk operations
  - Race conditions in constraint checking
  - Performance impact of constraint validation
  - Data corruption recovery scenarios

### 9.3 UTC/JST Time Conversion

#### Timezone Handling
- ✅ **Validation Steps**:
  - Store times in UTC, display in JST
  - Test daylight saving time transitions
  - Verify timezone conversion accuracy
  - Check historical time data integrity

- 📋 **Expected Behavior**:
  - All stored times in UTC for consistency
  - User-facing times displayed in JST
  - Accurate conversion accounting for DST
  - Time queries work correctly across timezones

- ⚠️ **Edge Cases**:
  - DST transition boundary times
  - Historical timezone rule changes
  - System timezone vs user timezone
  - Leap second handling (rare)

---

## 10. Continuous Integration & Quality

### 10.1 Build Process

#### Compilation & Dependencies
- ✅ **Validation Steps**:
  - Run `cargo build` on clean environment
  - Test with different Rust versions
  - Verify dependency resolution
  - Check build reproducibility

- 📋 **Expected Behavior**:
  - Clean builds succeed consistently
  - All dependencies resolve correctly
  - Build artifacts are consistent
  - No compiler warnings or errors

- ⚠️ **Edge Cases**:
  - Dependency version conflicts
  - Platform-specific build issues
  - Network failures during dependency fetch
  - Disk space issues during build

### 10.2 Test Coverage

#### Unit & Integration Tests
- ✅ **Validation Steps**:
  - Run `cargo test` and verify all tests pass
  - Check test coverage metrics
  - Test error scenarios and edge cases
  - Verify mock/fake implementations

- 📋 **Expected Behavior**:
  - All tests pass consistently
  - Good coverage of core functionality
  - Tests run quickly and reliably
  - Clear test failure messaging

- ⚠️ **Edge Cases**:
  - Flaky tests due to timing issues
  - Test database cleanup failures
  - Mock service limitations
  - Integration test environment differences

### 10.3 Code Quality

#### Linting & Static Analysis
- ✅ **Validation Steps**:
  - Run `cargo clippy` with zero warnings
  - Check code formatting with `cargo fmt`
  - Verify no security vulnerabilities
  - Test documentation generation

- 📋 **Expected Behavior**:
  - Code follows Rust style guidelines
  - No clippy warnings or errors
  - Consistent formatting throughout codebase
  - Comprehensive code documentation

- ⚠️ **Edge Cases**:
  - New clippy rules causing CI failures
  - Formatting conflicts between tools
  - False positive security warnings
  - Documentation generation failures

### 10.4 Migration Validation

#### Database Schema Checks
- ✅ **Validation Steps**:
  - Run `sqlx migrate run` in CI
  - Verify `cargo sqlx prepare --check` passes
  - Test migration idempotency
  - Check schema documentation accuracy

- 📋 **Expected Behavior**:
  - Migrations run successfully in CI
  - SQLx compile-time checks pass
  - Schema matches documentation
  - Migration files properly versioned

- ⚠️ **Edge Cases**:
  - Migration timing in CI environment
  - Database driver version differences
  - Migration file corruption
  - SQLx macro compilation issues

---

## Checklist Maintenance

### Regular Updates Required

1. **New Feature Addition**: Add validation steps for new features
2. **Bug Discovery**: Update edge cases and validation steps
3. **Dependencies Update**: Review impact on existing validations
4. **Discord API Changes**: Update interaction and message validations
5. **Performance Changes**: Adjust timing and scale expectations

### Automation Opportunities

- Automated testing for many validation steps
- CI integration for build/test/lint checks
- Monitoring alerts for production validation
- Performance regression testing
- Database integrity checks

### Review Schedule

- **Weekly**: Quick smoke tests of core functionality
- **Monthly**: Full checklist review for active features
- **Quarterly**: Complete end-to-end validation
- **Before releases**: All applicable sections
- **After incidents**: Affected feature areas

---

*Last updated: [Current Date]*
*Next review: [Next Review Date]*
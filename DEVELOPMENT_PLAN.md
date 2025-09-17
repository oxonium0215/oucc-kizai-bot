# OUCC Equipment Lending Bot - Development Plan

This document outlines the canonical feature set as defined by the official documentation and tracks implementation status. All future development should strictly follow this plan to maintain documentation-driven development.

## Documented Features Status

### ✅ IMPLEMENTED - Core Features

- **Setup Command** (`/setup`) - Bot configuration in any channel
- **Interactive Reservations** - Visual reservation system with modal forms and real-time conflict detection
- **Owner Transfer** - Transfer reservations between users with immediate and scheduled options
- **Managed Reservation Channels** - Fully automated equipment display with user message auto-deletion
- **Minimal API Updates** - Intelligent message editing minimizes Discord API usage and preserves message history
- **Time Zone Support** - All times displayed in JST (UTC+9) with automatic UTC conversion for storage
- **Equipment Organization** - Tag-based equipment categorization with custom sort orders
- **Permission Management** - User-level reservation management with admin override capabilities
- **Audit Logging** - Complete equipment operation history in equipment_logs table
- **Live Embed Updates** - Equipment availability refreshes automatically after reservation changes
- **Self-Healing** - Automatic message synchronization and repair on restart

### ✅ IMPLEMENTED - Notification System

- **Automated Reminders** - Via DM with channel fallback for reservation events
- **Reminder Types** - Pre-start, start, pre-end, overdue reminders
- **Delivery Methods** - Primary DM, fallback channel mentions
- **Configuration** - Configurable timing and frequency through `/setup`
- **Graceful Degradation** - Failed deliveries tracked, no spam retry

### ✅ IMPLEMENTED - Maintenance & Blackouts

- **Maintenance Windows** - Scheduled equipment downtime for cleaning, repairs, inspections
- **Conflict Prevention** - Prevents overlaps with reservations and other maintenance
- **Admin Management** - Create, edit, cancel maintenance through equipment embeds
- **Display Integration** - Current and upcoming maintenance shown in equipment embeds

### ✅ IMPLEMENTED - Overall Management

- **Admin Dashboard** - Comprehensive equipment and reservation management
- **Filter Controls** - Equipment, time, and status filtering
- **Bulk Actions** - Refresh display, CSV export
- **Pagination** - Handles large numbers of reservations
- **Access Control** - Admin-only with ephemeral interactions

### ✅ IMPLEMENTED - Technical Features

- **Database Schema** - SQLite with proper indexes and constraints
- **Background Jobs** - Reliable job queue for reminders and transfers
- **UTC Storage** - Consistent time handling with JST display
- **Conflict Detection** - Real-time validation prevents double-booking
- **Message Management** - Tracked Discord messages with reconciliation
- **Testing Suite** - Comprehensive unit, integration, and end-to-end tests

## Implementation Order for Future Development

All documented features are currently implemented. Any new feature requests must:

1. **First be documented** in the official README.md
2. **Have clear specifications** including UI mockups and behavior
3. **Follow the existing patterns** for consistency
4. **Include comprehensive tests** from the start
5. **Maintain backward compatibility** with existing data

## Out-of-Scope Features

The following features have been **REMOVED** from the codebase as they were not present in the official documentation:

- ❌ **User Quotas** - Per-guild and per-role reservation limits
- ❌ **Equipment Classes** - Class-based organization with specific limits  
- ❌ **Waitlist System** - Automatic offers when equipment becomes available
- ❌ **Class-Specific Quotas** - Different limits based on equipment class

## Next Steps for Contributors

1. **No new features should be added** without first updating the official documentation
2. **Bug fixes only** until documentation-driven development process is established
3. **Follow the validation checklist** in `docs/e2e-validation-checklist.md`
4. **Maintain test coverage** for all changes

## Ambiguities Requiring Clarification

Currently, all features in the official documentation have clear specifications. Any new ambiguities should be documented here as TODOs:

- TODO: None currently identified

## Architecture Decisions

- **SQLite Database** - Simple, reliable, file-based storage
- **UTC Internal Storage** - All times stored in UTC, displayed in JST
- **Discord Managed Messages** - Bot maintains full control over reservation channel content
- **Ephemeral Admin UI** - Admin interactions don't create channel noise
- **Background Job Processing** - Separate worker for scheduled tasks
- **Test-Driven Development** - Comprehensive test suite with mocked Discord API

## Maintenance Notes

- **Database Migrations** - Forward-only, safe migration strategy
- **Time Handling** - Careful conversion between UTC storage and JST display
- **Discord Rate Limits** - Intelligent batching and backoff strategies
- **Self-Healing** - Automatic recovery from Discord state inconsistencies

---

*Last Updated: November 2024*
*Next Review: When new features are proposed*
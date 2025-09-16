# E2E Validation Quick Reference

This is a condensed checklist for quick validation of the Equipment Lending Bot. For detailed validation steps, see [e2e-validation-checklist.md](./e2e-validation-checklist.md).

## Pre-deployment Quick Checks

### ✅ Core Setup
- [ ] `/setup` command works with admin permissions
- [ ] Channel auto-delete works for user messages
- [ ] Bot messages preserved in reservation channel
- [ ] Database migrations complete successfully

### ✅ Equipment Management
- [ ] Equipment embeds display correctly
- [ ] Equipment status updates work (Available/Loaned/Unavailable)
- [ ] Tag ordering and organization functional
- [ ] Overall Management UI accessible

### ✅ Reservations
- [ ] Reservation wizard completes successfully
- [ ] Conflict detection prevents double-booking
- [ ] Timezone conversion (UTC ↔ JST) accurate
- [ ] Reservation modification/cancellation works

### ✅ Notifications
- [ ] DM delivery working with fallback to channel mentions
- [ ] Reminder scheduling and delivery functional
- [ ] Transfer request notifications working

### ✅ Background Jobs
- [ ] Job queue processes scheduled tasks
- [ ] Retry logic works for failed jobs
- [ ] No duplicate jobs after restart

### ✅ Data Integrity
- [ ] Database constraints enforced
- [ ] Foreign key relationships maintained
- [ ] UTC time storage with JST display

### ✅ CI/Build
- [ ] `cargo build` succeeds
- [ ] `cargo test` passes all tests
- [ ] `cargo clippy` shows no warnings
- [ ] `sqlx migrate run` works in CI

## Smoke Test Sequence

1. **Setup Test** (5 min)
   - Run `/setup` in test channel
   - Confirm setup and verify channel behavior

2. **Equipment Test** (5 min)
   - Add test equipment via Overall Management
   - Verify embed appearance and interaction

3. **Reservation Test** (10 min)
   - Create reservation through wizard
   - Test conflict detection with overlapping times
   - Cancel reservation and verify status update

4. **Notification Test** (5 min)
   - Trigger DM notification
   - Test fallback behavior if DMs blocked

5. **Recovery Test** (5 min)
   - Restart bot (if possible)
   - Verify message synchronization

## Critical Edge Cases

### High Priority
- [ ] Restart during active user interaction
- [ ] Discord API rate limiting scenarios
- [ ] Database connection failures
- [ ] Permission changes mid-operation
- [ ] Timezone boundary conditions (midnight, DST)

### Medium Priority
- [ ] Very long equipment/location names
- [ ] Special characters in user inputs
- [ ] Concurrent admin modifications
- [ ] Large numbers of equipment items (>25)

## Failure Response

### If Core Features Fail
1. Check bot permissions in target channel
2. Verify database connectivity and schema
3. Review recent Discord API changes
4. Check error logs for specific failure points

### If Tests Fail
1. Run individual test files to isolate issues
2. Check database cleanup between tests
3. Verify environment variables and dependencies
4. Review recent code changes for regressions

## Automation Status

### ✅ Automated in CI
- Build and compilation
- Unit and integration tests
- Code formatting and linting
- Database migration validation

### 🔄 Semi-Automated
- Basic Discord interaction testing
- Database integrity checks
- Performance regression detection

### ❌ Manual Only
- End-to-end user workflows
- Discord permission testing
- Real-time notification delivery
- Cross-timezone functionality
- Admin UI interaction flows

## Update Schedule

- **Daily**: CI validation (automated)
- **Weekly**: Smoke test sequence (manual)
- **Before releases**: Full feature validation
- **After incidents**: Affected area re-validation

---

*For detailed validation procedures, see the full [End-to-End Validation Checklist](./e2e-validation-checklist.md)*
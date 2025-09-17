## Equipment Lending Management Bot Specification (Revision 2.3)

### **0. Initial Setup Function**

* **Purpose**:
    * To allow the administrator who introduces the bot to the server to easily and interactively complete the bot's essential settings using slash commands.

* **Command Specification**:
    * **Command**: `/setup`
    * **Description**: **Sets the channel where this command is executed as the "Reservation Channel"** and begins the bot's initial setup or configuration change.
    * **Execution Permissions**: Only executable by **users with administrator permissions on the server**.
    * **Interactive Format**: All interactions following command execution are conducted via **Ephemeral Messages (temporary messages)**.

* **Setup Flow**:
    1.  An administrator executes the `/setup` command **in the channel they wish to be the reservation channel**.
    2.  The bot first asks for confirmation with an Ephemeral Message: "**Set this channel as the reservation channel. Is that okay?**" and obtains consent.
    3.  After consent, the bot automatically checks if it has the necessary **permissions to send, receive, edit, and delete messages** in that channel. If permissions are insufficient, it displays an error message and aborts the process.
    4.  If there are no permission issues, it guides through the remaining settings via **step-by-step component input (wizard format)**.
    5.  **Step 1: Specifying Admin Roles (Multiple Selection Possible) (Optional)**
        * Presents a **role selection menu** and a `Skip` button along with the message "Please specify the roles that can use the bot's management functions."
        * Users with the roles specified here can use administrator functions like `Overall Management` similarly to server administrators.
    6.  **Step 2: Confirmation and Completion of Settings**
        * Finally, displays a list of the settings (Reservation Channel, Admin Roles) and requests final confirmation.
        * When the administrator presses the `Complete` button, the settings are saved to the database, and the setup is finished.
        * After completion, the bot posts the initial message ("Please register equipment," etc.) specified in specification `1.` to this channel and begins operation.

---

### 1. Reservation Status Visualization Function

* **Purpose**:
    * To constantly display the status of all managed equipment as individual messages in the `Reservation Channel`, making them viewable at a glance.
    * This channel is solely for the bot to display information. **Messages written by users are automatically deleted by the bot.**

* **Display When No Equipment is Registered**:
    * **Condition**: When the number of equipment managed by the bot is 0.
    * **Display Content**: Displays only one guide message in the channel saying "Please register equipment," with only the `Overall Management` button placed below it.

* **Display When One or More Equipment Items are Registered**:
    * The bot creates a dedicated embedded message (Embed) **for each piece** of managed equipment and continuously posts it to the channel.
    * **Display Order**: Equipment is grouped by **tag order**, and within the same tag, sorted by **equipment name order** (ascending).
    * Each equipment's Embed is individually edited whenever the reservation status is updated, ensuring it always reflects the latest state.
    * **Message Update & Order Maintenance**:
        * Based on an ideal order list stored in the database, the bot maintains the correct order and content by **sequentially editing** the existing messages in the channel.
        * If equipment is added or deleted, the bot adds or deletes messages, keeping the display count consistent with the equipment count.
        * This process achieves smooth updates with minimal API load and reduced screen flicker.

* **Embed Display Content for Each Equipment**:
    * **Tag** and equipment name (e.g., `[PC] PC-A`, `[VR] Oculus Quest 2`)
    * The equipment's current status (e.g., `Loaned (Clubroom) - User: XX`)
    * Current reservation list (Reserver name, start/end datetime)

* **Operation Button Placement**:
    * **Per-Equipment Operation Buttons**: Directly below each equipment's Embed, operation buttons linked to that equipment are placed. (e.g., `New Reservation`, `Return`, `Check/Change Reservation`, `This Equipment's Settings`)
    * **Overall Management Button**: Only one `Overall Management` button, independent of equipment, is placed **at the very top of the block of equipment list messages** posted by the bot.

---

### 2. Reservation & Lending Operation Functions

* **Common Interface**:
    * All interactions following button presses are conducted via **Ephemeral Messages (temporary messages)**.
    * Basically limits the interface displayed at one time to just one. If an existing interface is present, it is handled by editing it.

* **New Reservation / Reservation Change**:
    * Pressing the `New Reservation` button or selecting a reservation change operation causes the bot to send an Ephemeral Message and guide the user through **step-by-step component input (wizard format)**.
    * The user determines the reservation details by operating components (select menus, buttons) step-by-step, like Year/Month → Date → Start Time/Duration → Location.
    * **Input Correction Function**: To account for user input mistakes, each step of the wizard has a button **to return to the previous step** (e.g., `← Reselect Date`).
    * If the desired time slot is already reserved, the bot notifies the user at the time selection step: "**This time slot is already reserved. Please select another time.**"

* **Reservation Check/Change**:
    * **Access**: Press the `Check/Change Reservation` button below the desired equipment. Only accessible by the reserver themselves.
    * **Display Content**: Their own **confirmed reservations** for that equipment are listed in an Ephemeral Message.
    * **Possible Operations**:
        * **DateTime Change**: Changes the date/time using the wizard format UI described above.
        * **Reservation Cancellation**
        * **Reservation Owner Change**:
            * When a new owner is specified, **a DM is sent to that person requesting approval, and the owner only changes upon approval.**
            * Transfer requests are valid for **3 hours** and are automatically canceled if no response is received.
            * If denied by the recipient, the original requester is notified.
            * Only one valid transfer request per reservation is allowed at a time.

* **Return**:
    * Press the `Return` button below the message for the equipment to be returned and specify the return location in the displayed interface.
    * The default return location set for that equipment is pre-selected. (If not set, nothing is selected)
    * **If a location other than the default is selected, a confirmation message "Are you sure this location is correct?" is displayed to prevent mistakes.**

* **Return Correction/Cancellation**:
    * **Permission**: Can only be performed by the person who performed the return operation.
    * **Executable Conditions**: Executable until **the earlier of "1 hour after the return operation" and "15 minutes before the next reservation starts"**.
    * **Notification When Operation is Impossible**: If the above conditions are not met and the operation cannot be performed, a message is displayed: "**Operation is not possible because the next reservation is imminent or because 1 hour has passed since return.**"
    * **Function**: The following operations can be performed within the Ephemeral Message:
        * **Cancel Return**: Cancels the return operation, reverting the status back to "Loaned Out".
        * **Correct Return Location**: Corrects the registered return location to the correct one.

---

### 3. Notification & Reminder Function

* **Common Interface**:
    * All individual notifications from the bot to users are sent via **DM (Direct Message)**, clearly stating **which equipment the notification concerns**.
    * **Fallback for DM Send Failure**: If sending fails (e.g., user has DMs disabled), **sends a message mentioning the relevant user in the Lending Management Channel**, prompting them to enable DMs. If enabling DMs is confirmed, resends the notification.

* **Various Notifications**:
    * **15-Minute Pre-Reservation End Notification**: Sends a reminder notification to the user 15 minutes before the reservation end time. If no subsequent reservation exists at that time, also notifies about extension possibility, e.g., "**No next reservation, so extension is also possible.**"
    * **Return Delay Reminder**: If the return operation is not performed by the reservation end time, sends a reminder notification urging the user to extend or return.
    * **Reservation Conflict Notification**: If the previous user hasn't returned the equipment by the next reservation time, notifies **the next reserver** of the situation, e.g., "**The equipment is currently unavailable because the previous user has not returned it.**"
    * **Reservation Transfer Notification**: If a reservation ownership change request is "denied" or "canceled due to timeout," notifies the original requester via DM.

---

### 4. Management & Settings Functions

#### 4-1. Per-Equipment Settings Function

* **Access**:
    * Press the `This Equipment's Settings` button below the message for the equipment to configure. (Only visible/operable by administrators)
* **Main Functions (Operated within Ephemeral Message)**:
    * **Force State Change**: Manually changes the state of the selected equipment. If this change affects any reserved slots, the bot prompts the operator for final confirmation by showing the impact: "The following reservations will be automatically deleted. Proceed?" Upon approval, **the state is changed after notifying the original owners of the deleted reservations.**
    * **Set "Unavailable" Reason**: Sets/edits the reason for unavailability for the selected equipment.
    * **Rename Equipment**: Changes the name of the selected equipment.
    * **Assign Tag**: Assigns one tag from the managed tags to the selected equipment.
    * **Delete Equipment**: Removes the selected equipment from management.
    * **View This Equipment's Operation Log**: Select a period to view the operation log **limited to the selected equipment**.
    * **Set This Equipment's Default Return Location**: Sets one default return location specific to this equipment from the location list registered in `Overall Management`.

#### 4-2. Overall Management Function

* **Access**:
    * Press the `Overall Management` button at the top of the channel. (Only visible/operable by administrators)
* **Main Functions (Operated within Ephemeral Message)**:
    * **Add Equipment**: Registers the name of new equipment and selects **which tag it belongs to**.
    * **Manage Tags**: Adds, edits, deletes tags, and changes their display order.
    * **Manage Lending/Return Locations**: Manages the common location list for all equipment.
    * **View Overall Operation Log**: Select a period to view the operation logs for all equipment.
    * **Set Admin Roles**: Additionally specifies Discord roles that can use management functions. **(Note: Initial setup is done via the `/setup` command, but it's also possible to add/change them later from this screen.)**

---

### 5. Technical Specifications

* **Database**:
    * **Engine**: Uses **SQLite** to persist data such as equipment, reservations, settings, and logs.
    * **Per-Server Settings**: Saves and manages information like **Reservation Channel ID** and **Admin Role ID** set by the `/setup` command individually for each server the bot is added to.
    * **Concurrency**: Accesses the database via a **connection pool** provided by libraries like `sqlx` to avoid conflicts arising from Rust's async processing.
    * **Data Integrity**: Uses **database transactions** for critical operations like reservation processing to guarantee atomicity and prevent conflicts (Race Conditions).

* **Permission Management**:
    * **Default Administrators**: **Users with server administrator permissions**.
    * **Additional Administrators**: Allows optionally specifying **Discord roles with management permissions** via the `/setup` command or within the `Overall Management` function.

* **Specifications for Stable Operation**:
    * **Message Management**: The bot saves the ID of each equipment message it posts in the `Reservation Channel` to the database to identify managed objects.
    * **Synchronization on Restart**: Upon bot restart, reconciles database information with messages in the channel, performing reposting or deletion if discrepancies exist to synchronize the state.
    * **Self-Repair Function**: If it detects that the link between the database and channel message IDs is broken (e.g., message manually deleted), the bot **deletes all messages under its management once and rebuilds them based on the database**.

* **Time Zone**:
    * All time information is handled fixed to **JST (Japan Standard Time)**.

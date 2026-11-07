import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// Syncing a device with Garmin's inReach service, as inReach Sync does: what
// a sync would change, the sync itself and what it did.
ColumnLayout {
    id: section

    property var mode: null
    signal requestSignIn()
    signal switchOn()

    readonly property var preview: backend.preview
    readonly property var plan: preview.plan || null
    readonly property bool syncing: backend.task === "sync"
    readonly property bool checking: backend.task === "preview"
    readonly property bool showResult: !syncing && (Object.keys(backend.syncResult).length > 0 || backend.syncError.length > 0)

    spacing: 8

    // The core names its steps in English; each is translated here (an
    // unknown one shows as it comes).
    function stepText(step) {
        var known = {
            "Reading the inReach and Garmin's service": qsTr("Reading the inReach and Garmin's service"),
            "Telling Garmin the inReach's software version": qsTr("Telling Garmin the inReach's software version"),
            "Clearing the sync data": qsTr("Clearing the sync data"),
            "Writing the contacts": qsTr("Writing the contacts"),
            "Writing the preset messages": qsTr("Writing the preset messages"),
            "Writing the quick texts": qsTr("Writing the quick texts"),
            "Writing the settings": qsTr("Writing the settings"),
            "Syncing the waypoints": qsTr("Syncing the waypoints"),
            "Sending the tracks": qsTr("Sending the tracks"),
            "Syncing the routes": qsTr("Syncing the routes"),
            "Running Garmin's device commands": qsTr("Running Garmin's device commands"),
            "Writing the time of the sync": qsTr("Writing the time of the sync"),
            "Reading the inReach back": qsTr("Reading the inReach back"),
            "Restarting the inReach": qsTr("Restarting the inReach")
        }
        return known[step] !== undefined ? known[step] : step
    }

    function when(iso) {
        if (!iso)
            return qsTr("never")
        return new Date(iso).toLocaleString(Qt.locale(), Locale.ShortFormat)
    }

    // One line per kind of data: what a sync does to it.
    function lines(plan) {
        if (!plan)
            return []
        function changes(title, c) {
            if (c.left_alone)
                return { title: title, text: qsTr("not touched"), changed: false }
            var parts = []
            if (c.added.length)
                parts.push(qsTr("%1 to add").arg(c.added.length))
            if (c.removed.length)
                parts.push(qsTr("%1 to remove").arg(c.removed.length))
            parts.push(qsTr("%1 unchanged").arg(c.unchanged.length))
            return { title: title, text: parts.join(", "), changed: c.added.length + c.removed.length > 0 }
        }
        var out = [
            changes(qsTr("Contacts"), plan.contacts),
            changes(qsTr("Address book"), plan.addresses),
            changes(qsTr("Preset messages"), plan.presets),
            changes(qsTr("Quick texts"), plan.quick_texts)
        ]
        if (plan.device_contacts_kept > 0)
            out[0].text += " · " + qsTr("%1 made on the device kept").arg(plan.device_contacts_kept)
        if (plan.addresses_kept.length > 0)
            out[1].text += " · " + qsTr("%1 the device keeps").arg(plan.addresses_kept.length)
        var places = section.preview.places
        if (places) {
            out.push({ title: qsTr("Waypoints"), text: places.waypoints === null ? qsTr("not synced")
                     : qsTr("the device's %1 go to Garmin, which sends back its merged list").arg(places.waypoints), changed: false })
            out.push({ title: qsTr("Routes"), text: places.routes === null ? qsTr("not synced")
                     : qsTr("the device's %1 go to Garmin, which sends back its routes").arg(places.routes), changed: false })
            out.push({ title: qsTr("Tracks"), text: places.tracks_to_send === null ? qsTr("not synced")
                     : (places.tracks_to_send > 0 ? qsTr("%1 track blocks to send").arg(places.tracks_to_send) : qsTr("nothing new to send")),
                       changed: places.tracks_to_send > 0 })
        }
        var cmds = section.preview.device_commands
        if (cmds)
            out.push({ title: qsTr("Device commands"), text: cmds.length === 0 ? qsTr("none from Garmin")
                     : qsTr("%1 from Garmin, run as sent").arg(cmds.length), changed: cmds.length > 0 })
        var s = plan.settings
        if (s.answered)
            out.push({ title: qsTr("Settings"), text: s.changed.length ? qsTr("%1 to change, %2 unchanged").arg(s.changed.length).arg(s.unchanged)
                                                                    : qsTr("%1 unchanged").arg(s.unchanged), changed: s.changed.length > 0 })
        else
            out.push({ title: qsTr("Settings"), text: qsTr("none from Garmin"), changed: false })
        return out
    }

    function isChange(plan) {
        if (!plan)
            return false
        var any = false
        for (var l of lines(plan))
            any = any || l.changed
        return any || plan.other_sync_data_removed.length > 0 || plan.settings.device_type_change !== null
    }

    RowLayout {
        Layout.fillWidth: true
        Label {
            text: qsTr("Sync with Garmin")
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }
        Label {
            visible: section.plan !== null && !section.syncing
            text: qsTr("Last synced %1").arg(section.when(section.preview.last_synced))
            opacity: 0.6
        }
    }

    // Not signed in.
    RowLayout {
        visible: backend.account.length === 0
        Layout.fillWidth: true
        spacing: 12
        AccentButton {
            text: qsTr("Sign in…")
            onClicked: section.requestSignIn()
        }
        Label {
            text: qsTr("Contacts, preset messages, quick texts and settings come from your Garmin account (Explore).")
            opacity: 0.6
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    // Signed in, but the device is not on.
    RowLayout {
        visible: backend.account.length > 0 && section.mode !== "app" && !section.syncing && !section.showResult
        Layout.fillWidth: true
        spacing: 12
        Button {
            Material.roundedScale: theme.radius
            visible: section.mode === "charger"
            text: qsTr("Switch on")
            enabled: !backend.busy
            onClicked: section.switchOn()
        }
        Label {
            text: section.mode === "mass-storage"
                  ? qsTr("To sync, eject the GARMIN drive in the file manager; the inReach then charges, and can be switched on here.")
                  : qsTr("The inReach must be on to sync.")
            opacity: 0.6
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    // Asking.
    RowLayout {
        visible: section.checking || section.syncing
        spacing: 10
        BusyIndicator { running: visible; implicitWidth: 24; implicitHeight: 24 }
        Label {
            text: section.syncing ? (backend.syncStep.length ? qsTr("%1…").arg(section.stepText(backend.syncStep)) : qsTr("Syncing…"))
                                  : qsTr("Asking the inReach and Garmin…")
        }
    }
    Label {
        visible: section.syncing
        text: qsTr("Keep the inReach plugged in and on.")
        color: Material.color(Material.Orange, Material.Shade300)
    }

    Label {
        visible: backend.previewError.length > 0 && !section.checking && !section.syncing
        text: backend.previewError
        color: Material.color(Material.Red, Material.Shade300)
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    // What a sync would do.
    ColumnLayout {
        visible: section.plan !== null && !section.checking && !section.syncing && !section.showResult
        Layout.fillWidth: true
        spacing: 4

        Label {
            visible: !!section.preview.firmware_update
            text: section.preview.firmware_update
                  ? qsTr("Garmin offers software %1.%2.%3, which inReach Sync installs before syncing. nimrag-sync cannot install it this way yet, so it does not sync.")
                        .arg(section.preview.firmware_update.version_major).arg(section.preview.firmware_update.version_minor)
                        .arg(section.preview.firmware_update.version_rev)
                  : ""
            color: Material.color(Material.Orange, Material.Shade300)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        GridLayout {
            columns: 2
            columnSpacing: 16
            rowSpacing: 4
            Repeater {
                model: section.lines(section.plan)
                delegate: Label {
                    required property var modelData
                    required property int index
                    Layout.row: index
                    Layout.column: 0
                    Layout.alignment: Qt.AlignTop
                    text: modelData.title
                    opacity: 0.6
                }
            }
            Repeater {
                model: section.lines(section.plan)
                delegate: Label {
                    required property var modelData
                    required property int index
                    Layout.row: index
                    Layout.column: 1
                    Layout.fillWidth: true
                    text: modelData.text
                    color: modelData.changed ? theme.accent : Material.foreground
                    wrapMode: Text.Wrap
                }
            }
        }

        Label {
            visible: section.plan !== null && section.plan.settings.device_type_change !== null
            text: qsTr("Garmin's device type differs from the inReach's: syncing resets ALL its settings to their defaults first.")
            color: Material.color(Material.Red, Material.Shade300)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.topMargin: 6
            spacing: 12
            AccentButton {
                text: qsTr("Sync now")
                enabled: !backend.busy && !section.preview.firmware_update
                onClicked: backend.sync()
            }
            Button {
                Material.roundedScale: theme.radius
                text: qsTr("Check again")
                flat: true
                enabled: !backend.busy
                onClicked: backend.checkSync()
            }
            Label {
                text: section.isChange(section.plan) ? qsTr("A sync would make the changes above.")
                                                     : qsTr("Up to date. A sync writes the same again, as inReach Sync does.")
                opacity: 0.6
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }

    // Signed in and on, nothing asked yet.
    Button {
        Material.roundedScale: theme.radius
        visible: backend.account.length > 0 && section.mode === "app" && section.plan === null
                 && !section.checking && !section.syncing && !section.showResult && backend.previewError.length === 0
        text: qsTr("Check with Garmin")
        enabled: !backend.busy
        onClicked: backend.checkSync()
    }

    // What the last sync did.
    ColumnLayout {
        visible: section.showResult
        Layout.fillWidth: true
        spacing: 4
        Label {
            visible: backend.syncError.length > 0
            text: backend.syncError
            color: Material.color(Material.Red, Material.Shade300)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            visible: backend.syncError.length === 0
            text: {
                var r = backend.syncResult
                if (!r.left_over)
                    return ""
                return section.isChange(r.left_over) ? qsTr("Synced, but read back the inReach still differs from Garmin.")
                                                     : qsTr("Synced. Read back, the inReach matches Garmin.")
            }
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            visible: backend.syncError.length === 0
            text: backend.syncResult.mode_after_restart === "mass-storage"
                  ? qsTr("It restarted as a USB drive. Eject the GARMIN drive to use it again.")
                  : qsTr("It restarted.")
            opacity: 0.7
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            visible: !!backend.syncResult.backup
            text: qsTr("Its contacts, sync data and settings as they were are kept in %1").arg(backend.syncResult.backup || "")
            opacity: 0.5
            font.pixelSize: 12
            wrapMode: Text.WrapAnywhere
            Layout.fillWidth: true
        }
    }
}

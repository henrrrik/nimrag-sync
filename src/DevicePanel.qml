import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// One connected inReach: what it is, its software and mode, and what can be
// done with it now.
Rectangle {
    id: panel

    property var device: ({})
    property bool busy: false
    signal switchToDrive()
    signal switchOn()
    signal requestSignIn()

    readonly property var version: device.version || null
    readonly property var mode: device.mode === undefined ? null : device.mode
    readonly property bool isDrive: mode === "mass-storage"

    function modeText(m) {
        if (m === "app") return qsTr("On")
        if (m === "charger") return qsTr("Charging, switched off")
        if (m === "mass-storage") return qsTr("USB drive")
        if (m === "test") return qsTr("Test mode")
        if (m === "bootloader") return qsTr("Bootloader")
        return qsTr("Unknown")
    }

    color: "#18181c"
    radius: theme.radius
    implicitHeight: content.implicitHeight + 32

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: 16
        spacing: 8

        Label {
            text: panel.version ? panel.version.product : panel.device.name
            font.pixelSize: 18
            font.weight: Font.DemiBold
        }

        GridLayout {
            columns: 2
            columnSpacing: 16
            rowSpacing: 4
            visible: panel.version !== null

            Label { text: qsTr("Software"); opacity: 0.6 }
            Label { text: panel.version ? panel.version.firmware : "" }
            Label { text: qsTr("Mode"); opacity: 0.6 }
            Label { text: panel.modeText(panel.mode) }
        }

        Label {
            visible: !!panel.device.error
            text: panel.device.needs_permission
                  ? qsTr("No access to its USB interface. Add the udev rule from the README, then plug it in again.")
                  : (panel.device.error || "")
            color: Material.color(Material.Red, Material.Shade300)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            visible: panel.version !== null && !panel.isDrive
            Layout.fillWidth: true
            Layout.topMargin: 4
            spacing: 12

            AccentButton {
                text: qsTr("Switch to USB drive mode")
                enabled: !panel.busy
                onClicked: panel.switchToDrive()
            }
            Label {
                text: qsTr("It restarts. If it is switched on, it stops what it is doing, tracking too.")
                opacity: 0.6
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        Label {
            visible: panel.isDrive
            text: qsTr("Its files are available as the USB drive GARMIN.")
            opacity: 0.6
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        SoftwareSection {
            visible: panel.isDrive || backend.updateInstalled
            Layout.fillWidth: true
            Layout.topMargin: 4
        }

        Rectangle {
            visible: panel.version !== null
            color: "#2a2a30"
            implicitHeight: 1
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.bottomMargin: 4
        }

        SyncSection {
            visible: panel.version !== null
            mode: panel.mode
            Layout.fillWidth: true
            onRequestSignIn: panel.requestSignIn()
            onSwitchOn: panel.switchOn()
        }
    }
}

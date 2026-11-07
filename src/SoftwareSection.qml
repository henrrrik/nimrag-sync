import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// Software updates for an inReach in its USB drive mode: Garmin's update
// server asked with the drive's GarminDevice.xml, the update put on the drive
// and the drive ejected; the inReach installs it when it restarts.
ColumnLayout {
    id: section

    readonly property var check: backend.updateCheck
    readonly property var updates: check.updates || []
    readonly property bool checking: backend.task === "update-check"
    readonly property bool installing: backend.task === "update"

    spacing: 8

    // The core names its steps in English; each is translated here.
    function stepText(step) {
        if (step.indexOf("Downloading ") === 0)
            return qsTr("Downloading %1").arg(step.substring("Downloading ".length))
        var known = {
            "Asking Garmin for software updates": qsTr("Asking Garmin for software updates"),
            "Copying the update to the inReach": qsTr("Copying the update to the inReach"),
            "Ejecting the inReach's drive": qsTr("Ejecting the inReach's drive")
        }
        return known[step] !== undefined ? known[step] : step
    }

    function megabytes(bytes) {
        return (bytes / 1e6).toLocaleString(Qt.locale(), 'f', 1)
    }

    Label {
        text: qsTr("Software updates")
        font.weight: Font.DemiBold
    }

    RowLayout {
        visible: section.checking || section.installing
        spacing: 10
        BusyIndicator { running: visible; implicitWidth: 24; implicitHeight: 24 }
        Label {
            text: section.installing ? qsTr("%1…").arg(section.stepText(backend.updateStep || "Asking Garmin for software updates"))
                                     : qsTr("Asking Garmin for software updates…")
        }
    }
    ProgressBar {
        visible: section.installing && backend.updateStep.indexOf("Downloading ") === 0
        value: backend.updateProgress
        Layout.fillWidth: true
    }
    Label {
        visible: section.installing
        text: qsTr("Keep the inReach plugged in.")
        color: Material.color(Material.Orange, Material.Shade300)
    }

    Label {
        visible: backend.updateError.length > 0 && !section.checking && !section.installing
        text: backend.updateError
        color: Material.color(Material.Red, Material.Shade300)
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    Label {
        visible: backend.updateInstalled && !section.installing
        text: qsTr("The update is on the inReach and its drive is ejected. Leave it plugged in: it installs the update when it restarts.")
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    // What Garmin offers.
    ColumnLayout {
        visible: section.check.device !== undefined && !section.checking && !section.installing && !backend.updateInstalled
        Layout.fillWidth: true
        spacing: 6

        Label {
            visible: section.updates.length === 0
            text: qsTr("Software %1 is up to date.").arg(section.check.device ? section.check.device.software_version : "")
            opacity: 0.8
        }
        Repeater {
            model: section.updates
            delegate: Label {
                required property var modelData
                text: qsTr("%1 %2 is available (%3 MB).").arg(modelData.productName || modelData.partNumber)
                          .arg(modelData.version || "").arg(section.megabytes(modelData.deliverable.sizeInBytes))
                color: theme.accent
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
        RowLayout {
            visible: section.updates.length > 0
            spacing: 12
            AccentButton {
                text: qsTr("Install update")
                enabled: !backend.busy
                onClicked: backend.installUpdates()
            }
            Label {
                readonly property string license: section.updates.length > 0
                    ? (section.updates[0].secureEulaUrl || section.updates[0].eulaUrl || "") : ""
                text: qsTr("Installing it means accepting <a href=\"%1\">Garmin's license</a>.").arg(license)
                textFormat: Text.StyledText
                linkColor: theme.accent
                onLinkActivated: (link) => Qt.openUrlExternally(link)
                opacity: 0.7
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }

    Button {
        visible: !section.checking && !section.installing && !backend.updateInstalled
                 && (section.check.device === undefined || backend.updateError.length > 0)
        text: backend.updateError.length > 0 ? qsTr("Check again") : qsTr("Check for a software update")
        flat: true
        enabled: !backend.busy
        Material.roundedScale: theme.radius
        onClicked: backend.checkUpdates()
    }
}

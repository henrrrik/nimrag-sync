import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: win
    width: 760
    height: 560
    minimumWidth: 520
    minimumHeight: 400
    visible: true
    title: "nimrag-sync"
    color: "#0e0e10"

    Material.theme: Material.Dark
    Material.accent: theme.accent
    Material.roundedScale: theme.radius

    property bool helpVisible: false
    readonly property var shortcuts: [
        ["Space", qsTr("Ask the devices again")],
        ["R", qsTr("Ask the devices again")],
        ["C", qsTr("Check with Garmin what a sync would change")],
        ["?", qsTr("Show or hide the shortcuts")],
        ["Q", qsTr("Quit")]
    ]

    Shortcut { sequence: "?"; context: Qt.ApplicationShortcut; enabled: !signIn.visible; onActivated: win.helpVisible = !win.helpVisible }
    Shortcut { sequence: "Q"; context: Qt.ApplicationShortcut; enabled: !signIn.visible && backend.task !== "sync"; onActivated: Qt.quit() }
    Shortcut { sequences: ["Space", "R"]; context: Qt.ApplicationShortcut; enabled: !signIn.visible; onActivated: backend.refresh() }
    Shortcut { sequence: "C"; context: Qt.ApplicationShortcut; enabled: !signIn.visible && backend.account.length > 0; onActivated: backend.checkSync() }
    Shortcut { sequence: "Escape"; context: Qt.ApplicationShortcut; enabled: win.helpVisible; onActivated: win.helpVisible = false }

    Connections {
        target: backend
        function onNotice(text) { noticeLabel.show(text) }
    }

    // Right-to-left languages mirror the window; the Translator sets the
    // application's layout direction.
    Item {
        anchors.fill: parent
        LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
        LayoutMirroring.childrenInherit: true

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 24
            spacing: 16

            RowLayout {
                Layout.fillWidth: true
                Label {
                    text: qsTr("inReach devices")
                    font.pixelSize: 22
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                }
                BusyIndicator {
                    running: backend.busy
                    visible: backend.busy
                    implicitWidth: 28
                    implicitHeight: 28
                }
                Label {
                    visible: backend.account.length > 0
                    text: backend.account
                    opacity: 0.6
                }
                Button {
                    flat: true
                    text: backend.account.length > 0 ? qsTr("Sign out") : qsTr("Sign in…")
                    enabled: backend.task !== "sync"
                    onClicked: backend.account.length > 0 ? backend.signOut() : signIn.begin()
                }
                Label {
                    text: qsTr("? for shortcuts")
                    opacity: 0.5
                }
            }

            Label {
                visible: backend.error.length > 0
                text: backend.error
                color: Material.color(Material.Red, Material.Shade300)
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: backend.devices.length > 0
                contentWidth: availableWidth

                ColumnLayout {
                    width: parent.width
                    spacing: 12
                    Repeater {
                        model: backend.devices
                        delegate: DevicePanel {
                            required property var modelData
                            device: modelData
                            busy: backend.busy
                            Layout.fillWidth: true
                            onSwitchToDrive: backend.switchToDrive(modelData.node)
                            onSwitchOn: backend.switchOn(modelData.node)
                            onRequestSignIn: signIn.begin()
                        }
                    }
                }
            }

            ColumnLayout {
                visible: backend.devices.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 8
                Item { Layout.fillHeight: true }
                Label {
                    text: backend.busy ? qsTr("Looking for devices…") : qsTr("Connect your inReach")
                    font.pixelSize: 20
                    Layout.alignment: Qt.AlignHCenter
                }
                Label {
                    visible: !backend.busy
                    text: qsTr("Plug it in with a USB cable that carries data. It is found by itself.")
                    opacity: 0.7
                    wrapMode: Text.Wrap
                    horizontalAlignment: Text.AlignHCenter
                    Layout.fillWidth: true
                }
                Item { Layout.fillHeight: true }
            }
        }

        SignInDialog { id: signIn }

        Label {
            id: noticeLabel
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 20
            padding: 10
            opacity: 0
            background: Rectangle { color: "#26262b"; radius: Math.min(4, theme.radius) }

            function show(text) {
                noticeLabel.text = text
                noticeLabel.opacity = 1
                hideNotice.restart()
            }
            Behavior on opacity { NumberAnimation { duration: 200 } }
            Timer { id: hideNotice; interval: 4000; onTriggered: noticeLabel.opacity = 0 }
        }

        Rectangle {
            anchors.fill: parent
            visible: win.helpVisible
            color: "#cc0e0e10"
            MouseArea { anchors.fill: parent; onClicked: win.helpVisible = false }

            Rectangle {
                anchors.centerIn: parent
                width: Math.min(parent.width - 48, 420)
                height: helpColumn.implicitHeight + 40
                radius: theme.radius
                color: "#1a1a1e"

                ColumnLayout {
                    id: helpColumn
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 10
                    Label { text: qsTr("Shortcuts"); font.pixelSize: 18; font.weight: Font.DemiBold }
                    Repeater {
                        model: win.shortcuts
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Label {
                                text: modelData[0]
                                font.family: "monospace"
                                color: theme.accent
                                Layout.preferredWidth: 70
                            }
                            Label { text: modelData[1]; Layout.fillWidth: true; wrapMode: Text.Wrap }
                        }
                    }
                }
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// Signing in to the Garmin account the inReach is on (the one used on
// explore.garmin.com). The password goes to the desktop's keyring.
Dialog {
    id: dialog

    title: qsTr("Sign in to Garmin")
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(parent ? parent.width - 48 : 420, 420)
    Material.roundedScale: theme.radius

    property string failure: ""

    function begin() {
        failure = ""
        password.text = ""
        open()
        email.forceActiveFocus()
    }

    function submit() {
        if (email.text.trim().length === 0 || password.text.length === 0)
            return
        failure = ""
        backend.signIn(email.text, password.text)
    }

    Connections {
        target: backend
        function onSignInFinished(ok, error) {
            if (!dialog.visible)
                return
            if (ok) {
                password.text = ""
                dialog.close()
            } else {
                dialog.failure = error
            }
        }
    }

    contentItem: ColumnLayout {
        spacing: 12
        Label {
            text: qsTr("The Garmin account your inReach is on, as on explore.garmin.com. The password is kept in the desktop's keyring.")
            wrapMode: Text.Wrap
            opacity: 0.7
            Layout.fillWidth: true
        }
        TextField {
            Material.roundedScale: theme.radius
            id: email
            placeholderText: qsTr("Email address")
            inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoAutoUppercase
            Layout.fillWidth: true
            onAccepted: password.forceActiveFocus()
        }
        TextField {
            Material.roundedScale: theme.radius
            id: password
            placeholderText: qsTr("Password")
            echoMode: TextInput.Password
            Layout.fillWidth: true
            onAccepted: dialog.submit()
        }
        Label {
            visible: dialog.failure.length > 0
            text: dialog.failure
            color: Material.color(Material.Red, Material.Shade300)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    footer: DialogButtonBox {
        Button {
            Material.roundedScale: theme.radius
            text: qsTr("Cancel")
            flat: true
            DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
        }
        AccentButton {
            text: backend.task === "sign-in" ? qsTr("Signing in…") : qsTr("Sign in")
            enabled: backend.task !== "sign-in" && email.text.trim().length > 0 && password.text.length > 0
            onClicked: dialog.submit()
        }
    }
}

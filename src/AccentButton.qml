import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

// A button filled with the accent, its text black or white, whichever reads
// on it (theme.accentForeground). Material's highlighted button picks its text
// colour from a colour the app cannot set, white even on a light accent.
Button {
    id: control
    highlighted: true
    Material.roundedScale: theme.radius
    contentItem: Label {
        text: control.text
        font: control.font
        color: control.enabled ? theme.accentForeground : control.Material.hintTextColor
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
}

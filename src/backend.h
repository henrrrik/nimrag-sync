#pragma once

#include <QFutureWatcher>
#include <QObject>
#include <QStringList>
#include <QTimer>
#include <QVariantList>

// The window's view of the Rust core: the connected inReach devices and the
// actions on them. Asking a device takes up to a few seconds, so every call
// into the core that talks to a device runs on a worker thread.
class Backend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

public:
    explicit Backend(QObject *parent = nullptr, bool poll = true);

    QVariantList devices() const { return m_devices; }
    bool busy() const { return m_busy; }
    QString error() const { return m_error; }

    // Asks every connected device for its mode and version again.
    Q_INVOKABLE void refresh();
    // Restarts the inReach at node as a USB drive.
    Q_INVOKABLE void switchToDrive(const QString &node);

    // The core's JSON as QML values; an empty list for anything that is not an array.
    static QVariantList parseDevices(const QByteArray &json);
    // The device nodes in the core's JSON, sorted, to notice plugging and unplugging.
    static QStringList nodesOf(const QByteArray &json);

signals:
    void devicesChanged();
    void busyChanged();
    void errorChanged();
    // A short line for the window to show after an action.
    void notice(const QString &text);

private:
    void poll();
    void setBusy(bool busy);
    void setError(const QString &error);

    QVariantList m_devices;
    QStringList m_nodes;
    bool m_busy = false;
    bool m_refreshAgain = false;
    QString m_error;
    QTimer m_poll;
    QFutureWatcher<QByteArray> m_refreshWatcher;
    QFutureWatcher<QByteArray> m_switchWatcher;
};

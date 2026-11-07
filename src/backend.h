#pragma once

#include <QJsonObject>
#include <QFutureWatcher>
#include <QObject>
#include <QStringList>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <functional>

// The window's view of the Rust core: the connected inReach devices, the
// Garmin account, and a sync. Asking a device or Garmin's service takes from
// seconds up to a minute, so every such call into the core runs on a worker
// thread, one at a time.
class Backend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    // What the worker is doing: "", "refresh", "switch", "preview", "sync" or "sign-in".
    Q_PROPERTY(QString task READ task NOTIFY busyChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
    // The signed-in Garmin account's email address; empty when signed out.
    Q_PROPERTY(QString account READ account NOTIFY accountChanged)
    // The last preview of a sync (core's sync::Preview), or empty.
    Q_PROPERTY(QVariantMap preview READ preview NOTIFY previewChanged)
    Q_PROPERTY(QString previewError READ previewError NOTIFY previewChanged)
    // The step a running sync is at, and what the last sync did (core's sync::Applied).
    Q_PROPERTY(QString syncStep READ syncStep NOTIFY syncChanged)
    Q_PROPERTY(QVariantMap syncResult READ syncResult NOTIFY syncChanged)
    Q_PROPERTY(QString syncError READ syncError NOTIFY syncChanged)

public:
    explicit Backend(QObject *parent = nullptr, bool poll = true);

    QVariantList devices() const { return m_devices; }
    bool busy() const { return !m_task.isEmpty(); }
    QString task() const { return m_task; }
    QString error() const { return m_error; }
    QString account() const { return m_account; }
    QVariantMap preview() const { return m_preview; }
    QString previewError() const { return m_previewError; }
    QString syncStep() const { return m_syncStep; }
    QVariantMap syncResult() const { return m_syncResult; }
    QString syncError() const { return m_syncError; }

    // Asks every connected device for its mode and version again.
    Q_INVOKABLE void refresh();
    // Restarts the inReach at node as a USB drive, or in app mode (switched on).
    Q_INVOKABLE void switchToDrive(const QString &node);
    Q_INVOKABLE void switchOn(const QString &node);
    // Signs in to the Garmin account; signInFinished tells how it went.
    Q_INVOKABLE void signIn(const QString &email, const QString &password);
    Q_INVOKABLE void signOut();
    // What a sync would change, then the sync itself.
    Q_INVOKABLE void checkSync();
    Q_INVOKABLE void sync();

    // The core's JSON as QML values; an empty list for anything that is not an array.
    static QVariantList parseDevices(const QByteArray &json);
    // The device nodes in the core's JSON, sorted, to notice plugging and unplugging.
    static QStringList nodesOf(const QByteArray &json);
    // Whether any device in the list is on (app mode), which a sync needs.
    static bool anyOn(const QVariantList &devices);

signals:
    void devicesChanged();
    void busyChanged();
    void errorChanged();
    void accountChanged();
    void previewChanged();
    void syncChanged();
    void signInFinished(bool ok, const QString &error);
    // A short line for the window to show after an action.
    void notice(const QString &text);

private:
    void poll();
    // Runs work() on a worker thread as `task`, then done() with its JSON object here.
    bool start(const QString &task, std::function<QByteArray()> work, std::function<void(const QJsonObject &)> done);
    void finish();
    void setError(const QString &error);
    void loadAccount();
    static void onStep(const char *step, void *context);

    QVariantList m_devices;
    QStringList m_nodes;
    QString m_task;
    bool m_refreshAgain = false;
    QString m_error;
    QString m_account;
    QVariantMap m_preview;
    QString m_previewError;
    QString m_syncStep;
    QVariantMap m_syncResult;
    QString m_syncError;
    QTimer m_poll;
};

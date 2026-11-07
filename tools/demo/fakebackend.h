// The window's Backend with made-up state, for screenshots: the same
// properties, signals and actions, no device and no network. Keep it in step
// with src/backend.h; tests/ checks that the properties match.
#pragma once
#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QVariantMap>
class FakeBackend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantList devices MEMBER m_devices NOTIFY changed)
    Q_PROPERTY(bool busy READ busy NOTIFY changed)
    Q_PROPERTY(QString task MEMBER m_task NOTIFY changed)
    Q_PROPERTY(QString error MEMBER m_error NOTIFY changed)
    Q_PROPERTY(QString account MEMBER m_account NOTIFY changed)
    Q_PROPERTY(QVariantMap preview MEMBER m_preview NOTIFY changed)
    Q_PROPERTY(QString previewError MEMBER m_previewError NOTIFY changed)
    Q_PROPERTY(QString syncStep MEMBER m_syncStep NOTIFY changed)
    Q_PROPERTY(QVariantMap syncResult MEMBER m_syncResult NOTIFY changed)
    Q_PROPERTY(QString syncError MEMBER m_syncError NOTIFY changed)
    Q_PROPERTY(QVariantMap updateCheck MEMBER m_updateCheck NOTIFY changed)
    Q_PROPERTY(QString updateStep MEMBER m_updateStep NOTIFY changed)
    Q_PROPERTY(double updateProgress MEMBER m_updateProgress NOTIFY changed)
    Q_PROPERTY(QString updateError MEMBER m_updateError NOTIFY changed)
    Q_PROPERTY(bool updateInstalled MEMBER m_updateInstalled NOTIFY changed)
public:
    explicit FakeBackend(const QByteArray &json) {
        const QVariantMap m = QJsonDocument::fromJson(json).object().toVariantMap();
        m_devices = m.value("devices").toList();
        m_task = m.value("task").toString();
        m_account = m.value("account").toString();
        m_preview = m.value("preview").toMap();
        m_syncStep = m.value("syncStep").toString();
        m_syncResult = m.value("syncResult").toMap();
        m_updateCheck = m.value("updateCheck").toMap();
        m_updateStep = m.value("updateStep").toString();
        m_updateProgress = m.value("updateProgress").toDouble();
    }
    bool busy() const { return !m_task.isEmpty(); }
    Q_INVOKABLE void refresh() {}
    Q_INVOKABLE void switchToDrive(const QString &) {}
    Q_INVOKABLE void switchOn(const QString &) {}
    Q_INVOKABLE void signIn(const QString &, const QString &) {}
    Q_INVOKABLE void signOut() {}
    Q_INVOKABLE void checkSync() {}
    Q_INVOKABLE void sync() {}
    Q_INVOKABLE void checkUpdates() {}
    Q_INVOKABLE void installUpdates() {}
signals:
    void changed();
    void notice(const QString &text);
    void signInFinished(bool ok, const QString &error);
private:
    QVariantList m_devices;
    QString m_task, m_error, m_account, m_previewError, m_syncStep, m_syncError;
    QVariantMap m_preview, m_syncResult, m_updateCheck;
    QString m_updateStep, m_updateError;
    double m_updateProgress = 0;
    bool m_updateInstalled = false;
};

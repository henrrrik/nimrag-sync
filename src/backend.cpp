#include "backend.h"

#include <QJsonArray>
#include <QJsonValue>
#include <QJsonDocument>
#include <QtConcurrent>

#include "nimrag.h"

namespace {

// Takes a string from the core and gives it back once copied.
QByteArray take(char *text) {
    QByteArray copy(text ? text : "");
    nimrag_string_free(text);
    return copy;
}

QJsonObject object(const QByteArray &json) {
    return QJsonDocument::fromJson(json).object();
}

} // namespace

Backend::Backend(QObject *parent, bool poll) : QObject(parent) {
    loadAccount();
    // Reading /sys is cheap, so it is polled; a device is asked only when the
    // set of connected devices changes, which includes its restarts (it
    // leaves the bus and comes back).
    m_poll.setInterval(2000);
    connect(&m_poll, &QTimer::timeout, this, &Backend::poll);
    if (poll) {
        m_poll.start();
        refresh();
    }
}

Backend::~Backend() {
    m_poll.stop();
    m_running.waitForFinished();
}

QVariantList Backend::parseDevices(const QByteArray &json) {
    const QJsonDocument document = QJsonDocument::fromJson(json);
    return document.isArray() ? document.array().toVariantList() : QVariantList{};
}

QStringList Backend::nodesOf(const QByteArray &json) {
    QStringList nodes;
    for (const auto &device : parseDevices(json))
        nodes << device.toMap().value("node").toString();
    nodes.sort();
    return nodes;
}

bool Backend::anyDrive(const QVariantList &devices) {
    for (const auto &device : devices)
        if (device.toMap().value("mode").toString() == "mass-storage")
            return true;
    return false;
}

bool Backend::anyOn(const QVariantList &devices) {
    for (const auto &device : devices)
        if (device.toMap().value("mode").toString() == "app")
            return true;
    return false;
}

void Backend::loadAccount() {
    const QString email = object(take(nimrag_account_json())).value("email").toString();
    if (email != m_account) {
        m_account = email;
        emit accountChanged();
    }
}

void Backend::poll() {
    const QStringList nodes = nodesOf(take(nimrag_devices_json(0)));
    if (nodes == m_nodes)
        return;
    m_nodes = nodes;
    // What was seen of an unplugged or restarted device is no longer true.
    if (!m_preview.isEmpty() || !m_previewError.isEmpty()) {
        m_preview.clear();
        m_previewError.clear();
        emit previewChanged();
    }
    // Every device unplugged: an update installed earlier was another's (or
    // done), so the next device gets its own check. Not on a mere restart,
    // which the installing device goes through.
    const bool forgetInstalled = m_updateInstalled && nodes.isEmpty();
    if (!m_updateCheck.isEmpty() || !m_updateError.isEmpty() || forgetInstalled) {
        m_updateCheck.clear();
        m_updateError.clear();
        if (forgetInstalled)
            m_updateInstalled = false;
        emit updateChanged();
    }
    refresh();
}

bool Backend::start(const QString &task, std::function<QByteArray()> work, std::function<void(const QJsonObject &)> done) {
    if (busy())
        return false;
    m_task = task;
    emit busyChanged();
    auto *watcher = new QFutureWatcher<QByteArray>(this);
    connect(watcher, &QFutureWatcher<QByteArray>::finished, this, [this, watcher, done] {
        const QByteArray result = watcher->result();
        watcher->deleteLater();
        m_task.clear();
        emit busyChanged();
        done(object(result));
        finish();
    });
    m_running = QtConcurrent::run(std::move(work));
    watcher->setFuture(m_running);
    return true;
}

// A device came or went during a task: ask again once it is done.
void Backend::finish() {
    if (m_refreshAgain && !busy()) {
        m_refreshAgain = false;
        refresh();
    }
}

void Backend::refresh() {
    if (busy()) {
        m_refreshAgain = true;
        return;
    }
    m_task = "refresh";
    emit busyChanged();
    auto *watcher = new QFutureWatcher<QByteArray>(this);
    connect(watcher, &QFutureWatcher<QByteArray>::finished, this, [this, watcher] {
        m_devices = parseDevices(watcher->result());
        watcher->deleteLater();
        emit devicesChanged();
        m_task.clear();
        emit busyChanged();
        finish();
        // Signed in and a device is on: show at once what a sync would do.
        if (!busy() && !m_account.isEmpty() && anyOn(m_devices) && m_preview.isEmpty() && m_previewError.isEmpty())
            checkSync();
        // A device is a USB drive: ask Garmin for its software updates at once.
        else if (!busy() && anyDrive(m_devices) && m_updateCheck.isEmpty() && m_updateError.isEmpty() && !m_updateInstalled)
            checkUpdates();
    });
    m_running = QtConcurrent::run([] { return take(nimrag_devices_json(1)); });
    watcher->setFuture(m_running);
}

void Backend::switchToDrive(const QString &node) {
    const QByteArray path = node.toUtf8();
    start("switch", [path] { return take(nimrag_switch_to_drive(path.constData())); }, [this](const QJsonObject &answer) {
        if (answer.contains("error"))
            return setError(answer.value("error").toString());
        setError({});
        emit notice(answer.value("switched").toBool() ? tr("The inReach restarts as a USB drive…")
                                                      : tr("The inReach is a USB drive already."));
    });
}

void Backend::switchOn(const QString &node) {
    const QByteArray path = node.toUtf8();
    start("switch", [path] { return take(nimrag_switch_on(path.constData())); }, [this](const QJsonObject &answer) {
        if (answer.contains("error"))
            return setError(answer.value("error").toString());
        setError({});
        emit notice(answer.value("restarted").toBool() ? tr("The inReach restarts and switches on…")
                                                       : tr("The inReach is on already."));
    });
}

void Backend::signIn(const QString &email, const QString &password) {
    const QByteArray e = email.toUtf8(), p = password.toUtf8();
    const bool started = start("sign-in", [e, p] { return take(nimrag_sign_in(e.constData(), p.constData())); },
                               [this](const QJsonObject &answer) {
                                   if (answer.contains("error"))
                                       return emit signInFinished(false, answer.value("error").toString());
                                   loadAccount();
                                   emit signInFinished(true, {});
                                   if (anyOn(m_devices))
                                       checkSync();
                               });
    if (!started)
        emit signInFinished(false, tr("Wait until the inReach is done, then try again."));
}

void Backend::signOut() {
    const QJsonObject answer = object(take(nimrag_sign_out()));
    if (answer.contains("error"))
        return setError(answer.value("error").toString());
    loadAccount();
    m_preview.clear();
    m_previewError.clear();
    emit previewChanged();
}

void Backend::checkSync() {
    start("preview", [] { return take(nimrag_preview_json()); }, [this](const QJsonObject &answer) {
        m_previewError = answer.value("error").toString();
        m_preview = answer.value("preview").toObject().toVariantMap();
        emit previewChanged();
    });
}

void Backend::onStep(const char *step, void *context) {
    // Called on the worker thread: hand the text to the window's thread.
    auto *self = static_cast<Backend *>(context);
    const QString text = QString::fromUtf8(step);
    QMetaObject::invokeMethod(self, [self, text] {
        self->m_syncStep = text;
        emit self->syncChanged();
    }, Qt::QueuedConnection);
}

void Backend::sync() {
    m_syncResult.clear();
    m_syncError.clear();
    m_syncStep.clear();
    emit syncChanged();
    start("sync", [this] { return take(nimrag_sync_apply(&Backend::onStep, this)); }, [this](const QJsonObject &answer) {
        m_syncStep.clear();
        m_syncError = answer.value("error").toString();
        m_syncResult = answer.value("applied").toObject().toVariantMap();
        emit syncChanged();
        // The preview was of the device before the sync.
        m_preview.clear();
        m_previewError.clear();
        emit previewChanged();
        if (m_syncError.isEmpty())
            emit notice(tr("Synced with Garmin."));
    });
}

void Backend::checkUpdates() {
    start("update-check", [] { return take(nimrag_update_check_json(0)); }, [this](const QJsonObject &answer) {
        m_updateError = answer.value("error").toString();
        m_updateCheck = answer.value("check").toObject().toVariantMap();
        emit updateChanged();
    });
}

void Backend::onUpdateStep(const char *step, void *context) {
    auto *self = static_cast<Backend *>(context);
    const QString text = QString::fromUtf8(step);
    QMetaObject::invokeMethod(self, [self, text] {
        self->m_updateStep = text;
        self->m_updateProgress = 0;
        emit self->updateChanged();
    }, Qt::QueuedConnection);
}

void Backend::onUpdateProgress(unsigned long long done, unsigned long long total, void *context) {
    auto *self = static_cast<Backend *>(context);
    const double share = total ? double(done) / double(total) : 0;
    QMetaObject::invokeMethod(self, [self, share] {
        // Only steps a bar can show, not every 64 KB.
        if (share - self->m_updateProgress >= 0.01 || share >= 1) {
            self->m_updateProgress = share;
            emit self->updateChanged();
        }
    }, Qt::QueuedConnection);
}

void Backend::installUpdates() {
    m_updateError.clear();
    m_updateStep.clear();
    m_updateProgress = 0;
    emit updateChanged();
    start("update", [this] { return take(nimrag_update_install(0, &Backend::onUpdateStep, &Backend::onUpdateProgress, this)); },
          [this](const QJsonObject &answer) {
              m_updateStep.clear();
              m_updateError = answer.value("error").toString();
              m_updateInstalled = m_updateError.isEmpty() && !answer.value("installed").toObject().value("files").toArray().isEmpty();
              m_updateCheck.clear();
              emit updateChanged();
              if (m_updateInstalled)
                  emit notice(tr("The update is on the inReach; it installs it when it restarts."));
          });
}

void Backend::setError(const QString &error) {
    if (error == m_error)
        return;
    m_error = error;
    emit errorChanged();
}

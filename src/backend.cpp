#include "backend.h"

#include <QJsonArray>
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
    watcher->setFuture(QtConcurrent::run(std::move(work)));
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
    });
    watcher->setFuture(QtConcurrent::run([] { return take(nimrag_devices_json(1)); }));
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

void Backend::setError(const QString &error) {
    if (error == m_error)
        return;
    m_error = error;
    emit errorChanged();
}

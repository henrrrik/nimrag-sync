#include "backend.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QtConcurrent>

#include "nimrag.h"

namespace {

// Takes a string from the core and gives it back once copied.
QByteArray take(char *text) {
    QByteArray copy(text ? text : "");
    nimrag_string_free(text);
    return copy;
}

} // namespace

Backend::Backend(QObject *parent, bool poll) : QObject(parent) {
    connect(&m_refreshWatcher, &QFutureWatcher<QByteArray>::finished, this, [this] {
        m_devices = parseDevices(m_refreshWatcher.result());
        emit devicesChanged();
        setBusy(false);
        if (m_refreshAgain) {
            m_refreshAgain = false;
            refresh();
        }
    });
    connect(&m_switchWatcher, &QFutureWatcher<QByteArray>::finished, this, [this] {
        const QJsonObject answer = QJsonDocument::fromJson(m_switchWatcher.result()).object();
        setBusy(false);
        if (answer.contains("error")) {
            setError(answer.value("error").toString());
            return;
        }
        setError({});
        emit notice(answer.value("switched").toBool() ? tr("The inReach restarts as a USB drive…")
                                                      : tr("The inReach is a USB drive already."));
    });

    // Reading /sys is cheap, so it is polled; a device is asked only when the
    // set of connected devices changes, which includes the restart into the
    // drive mode (the device leaves the bus and comes back).
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

void Backend::poll() {
    const QStringList nodes = nodesOf(take(nimrag_devices_json(0)));
    if (nodes == m_nodes)
        return;
    m_nodes = nodes;
    refresh();
}

void Backend::refresh() {
    if (m_busy) {
        // A device came or went during an action: ask again once it is done.
        m_refreshAgain = true;
        return;
    }
    setBusy(true);
    m_refreshWatcher.setFuture(QtConcurrent::run([] { return take(nimrag_devices_json(1)); }));
}

void Backend::switchToDrive(const QString &node) {
    if (m_busy)
        return;
    setBusy(true);
    const QByteArray path = node.toUtf8();
    m_switchWatcher.setFuture(QtConcurrent::run([path] { return take(nimrag_switch_to_drive(path.constData())); }));
}

void Backend::setBusy(bool busy) {
    if (busy == m_busy)
        return;
    m_busy = busy;
    emit busyChanged();
}

void Backend::setError(const QString &error) {
    if (error == m_error)
        return;
    m_error = error;
    emit errorChanged();
}

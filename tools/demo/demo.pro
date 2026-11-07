# The window with made-up state, for screenshots (bin/screenshot): the real
# QML, theme and translator, a FakeBackend in place of the Rust core.
QT += core gui qml quick quickcontrols2 dbus
CONFIG += c++17 release
TARGET = nimrag-demo
TEMPLATE = app

INCLUDEPATH += $$PWD/../../src
HEADERS += fakebackend.h $$PWD/../../src/theme.h $$PWD/../../src/translator.h
SOURCES += demo.cpp $$PWD/../../src/theme.cpp $$PWD/../../src/translator.cpp
RESOURCES += $$PWD/../../src/resources.qrc

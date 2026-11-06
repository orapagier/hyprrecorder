// Thin native view. Rust validates/persists settings and owns the media process.
#include <QtWidgets/QApplication>
#include <QtWidgets/QCheckBox>
#include <QtWidgets/QComboBox>
#include <QtWidgets/QFileDialog>
#include <QtWidgets/QFormLayout>
#include <QtWidgets/QHBoxLayout>
#include <QtWidgets/QLabel>
#include <QtWidgets/QLineEdit>
#include <QtWidgets/QPushButton>
#include <QtWidgets/QScrollArea>
#include <QtWidgets/QScrollBar>
#include <QtWidgets/QSpinBox>
#include <QtWidgets/QVBoxLayout>
#include <QtCore/QDir>
#include <QtCore/QFileInfo>
#include <QtGui/QIntValidator>
#include <QtGui/QResizeEvent>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QLockFile>
#include <QtCore/QProcess>
#include <QtCore/QTimer>
#include <QtNetwork/QLocalServer>
#include <QtNetwork/QLocalSocket>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <csignal>
#include <cstdio>
#include <unistd.h>
#include <cmath>

// Let the compositor own window geometry; only reflow the contents on resize.
class RecorderWindow : public QWidget {
public:
    QFormLayout *form=nullptr;
    QVBoxLayout *outer=nullptr;
protected:
    void resizeEvent(QResizeEvent *event) override {
        QWidget::resizeEvent(event);
        const bool compact=event->size().width()<560;
        if(form)form->setRowWrapPolicy(compact?QFormLayout::WrapAllRows:QFormLayout::DontWrapRows);
        if(outer)outer->setContentsMargins(compact?12:20,12,compact?12:20,12);
    }
};

extern "C" int hr_ui_run(const char *initial,const char *runtime,const char *binary,const char *settings_path,int smoke,int (*save)(const char *,char *,size_t),int (*active)())  {
    int argc=1;
    char name[]="hyprrecorder";
    char *argv[]= {
        name,nullptr
    };
    QApplication app(argc,argv);
    app.setApplicationName("Hyprrecorder");
    app.setDesktopFileName("io.github.hyprrecorder");
    app.setQuitOnLastWindowClosed(false);
    app.setStyle("Fusion");
    QFont font=app.font();
    font.setFamily("Adwaita Sans");
    app.setFont(font);
    app.setStyleSheet("QWidget{background:#202020;color:#f5f5f5;} QLabel#muted{color:#b8b8b8;} QLineEdit,QComboBox,QSpinBox{background:#252525;border:1px solid #484848;border-radius:8px;padding:7px;} QPushButton{background:#303030;border:1px solid #484848;border-radius:8px;padding:9px;} QPushButton#primary{background:#2874cc;border-color:#2874cc;} QPushButton:focus,QLineEdit:focus,QComboBox:focus,QSpinBox:focus,QCheckBox:focus{border:2px solid #78b7ff;} QPushButton:disabled{color:#888888;} QCheckBox{spacing:8px;padding:5px;} QCheckBox::indicator{width:18px;height:18px;} QScrollArea{border:0;}"
        "QScrollBar:vertical{background:transparent;width:8px;margin:0;}"
        "QScrollBar::handle:vertical{background:#555555;border-radius:4px;min-height:28px;}"
        "QScrollBar::handle:vertical:hover{background:#777777;}"
        "QScrollBar::add-line:vertical,QScrollBar::sub-line:vertical{height:0;}"
        "QScrollBar::add-page:vertical,QScrollBar::sub-page:vertical{background:transparent;}");
    QString root=QString::fromUtf8(runtime),exe=QString::fromUtf8(binary),config=QString::fromUtf8(settings_path);
    QLockFile lock(root+"/ui.lock");
    if(!smoke && !lock.tryLock(0))  {
        QLocalSocket socket;
        socket.connectToServer(root+"/ui.sock");
        if(!socket.waitForConnected(1000))return 3;
        socket.write("show\n");
        socket.waitForBytesWritten(1000);
        return 0;
    } QLocalServer server;
    if(!smoke)  {
        struct stat st;
        QByteArray path=(root+"/ui.sock").toUtf8();
        if(lstat(path.constData(),&st)==0 && !S_ISSOCK(st.st_mode))return 4;
        QLocalServer::removeServer(root+"/ui.sock");
        server.setSocketOptions(QLocalServer::UserAccessOption);
        if(!server.listen(root+"/ui.sock"))return 4;
    } QJsonObject startup=QJsonDocument::fromJson(initial).object(),original=startup["settings"].toObject();
    QJsonArray monitors=startup["monitors"].toArray();
    RecorderWindow window;
    window.setWindowTitle("Hyprrecorder");
    window.setMinimumSize(300,220);
    window.resize(560,680);
    auto *outer=new QVBoxLayout(&window);
    window.outer=outer;
    outer->setContentsMargins(20,18,20,18);
    outer->setSpacing(12);
    auto *scroll=new QScrollArea;
    scroll->setWidgetResizable(true);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *body=new QWidget;
    auto *form=new QFormLayout(body);
    window.form=form;
    form->setContentsMargins(0,0,8,8);
    form->setHorizontalSpacing(18);
    form->setVerticalSpacing(10);
    form->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);
    form->setFormAlignment(Qt::AlignTop);
    scroll->setWidget(body);
    outer->addWidget(scroll,1);
    auto wrapLabel=[](QLabel *label) {
        label->setWordWrap(true);
        label->setMinimumWidth(0);
        label->setSizePolicy(QSizePolicy::Ignored,QSizePolicy::Preferred);
        label->setTextFormat(Qt::PlainText);
    };
    auto section=[&](const QString &text) {
        auto *label=new QLabel(text);
        QFont sectionFont=font;
        sectionFont.setBold(true);
        label->setFont(sectionFont);
        label->setContentsMargins(0,12,0,0);
        form->addRow(label);
    };
    auto *title=new QLabel("Record your screen");
    QFont heading=font;
    heading.setPointSizeF(font.pointSizeF()*1.5);
    heading.setBold(true);
    title->setFont(heading);
    wrapLabel(title);
    form->addRow(title);
    auto *subtitle=new QLabel("Choose a screen or region. Controls hide before recording.");
    subtitle->setObjectName("muted");
    wrapLabel(subtitle);
    form->addRow(subtitle);
    section("Capture");
    auto *mode=new QComboBox;
    mode->addItem("Screen","screen");
    mode->addItem("Region","region");
    mode->setAccessibleName("Capture mode");
    auto capture=original["capture"].toObject();
    mode->setCurrentIndex(capture["mode"].toString()=="region"?1:0);
    form->addRow("Source",mode);
    auto *monitor=new QComboBox;
    monitor->setAccessibleName("Monitor");
    int focused=0;
    for(int i=0;i<monitors.size();i++) {
        auto m=monitors[i].toObject();
        monitor->addItem(m["name"].toString()+QString(" · %1 × %2").arg(m["width"].toInt()).arg(m["height"].toInt()),m["name"].toString());
        if(m["focused"].toBool())focused=i;
    } int selected=monitor->findData(capture["monitor"].toString());
    monitor->setCurrentIndex(selected>=0?selected:focused);
    if(monitors.size()>1)form->addRow("Monitor",monitor);
    else monitor->hide();
    auto *regionButton=new QPushButton("Select region…");
    regionButton->setEnabled(!monitors.isEmpty() && mode->currentIndex()==1);
    form->addRow("Region",regionButton);
    QJsonObject region=capture["region"].toObject();
    section("Video");
    auto *resolution=new QComboBox;
    resolution->setEditable(true);
    resolution->setAccessibleName("Output resolution");
    for(auto pair: {
        std::pair<const char*,const char*> {
            "Native","native"
        }, {
            "720p · 1280 × 720","1280x720"
        }, {
            "1080p · 1920 × 1080","1920x1080"
        }, {
            "1440p · 2560 × 1440","2560x1440"
        }, {
            "2160p · 3840 × 2160","3840x2160"
        }
    })resolution->addItem(pair.first,pair.second);
    auto video=original["video"].toObject();
    QString value=video["resolution"].toString();
    int idx=resolution->findData(value);
    if(idx>=0)resolution->setCurrentIndex(idx);
    else resolution->setEditText(value);
    form->addRow("Resolution",resolution);
    auto *fps=new QComboBox;
    fps->setEditable(true);
    fps->addItems( {
        "24","30","60"
    });
    fps->setCurrentText(QString::number(video["fps"].toInt(30)));
    fps->lineEdit()->setValidator(new QIntValidator(1,240,fps));
    fps->setAccessibleName("Frames per second, 1 to 240");
    form->addRow("Frames / second",fps);
    auto *cursor=new QCheckBox("Include cursor");
    cursor->setChecked(video["cursor"].toBool(true));
    form->addRow(cursor);
    section("Audio");
    auto audio=original["audio"].toObject();
    auto *system=new QCheckBox("System audio");
    system->setChecked(audio["system"].toBool(true));
    form->addRow(system);
    auto *mic=new QCheckBox("Microphone");
    mic->setChecked(audio["microphone"].toBool(true));
    form->addRow(mic);
    auto *audioHelp=new QLabel("Uses the default output and microphone at start. With speakers, the microphone may also pick up system sound.");
    wrapLabel(audioHelp);
    audioHelp->setObjectName("muted");
    form->addRow(audioHelp);
    section("Save recording");
    auto *directory=new QLineEdit(original["output"].toObject()["directory"].toString());
    directory->setAccessibleName("Save directory");
    directory->setMinimumWidth(0);
    directory->setSizePolicy(QSizePolicy::Ignored,QSizePolicy::Fixed);
    directory->setToolTip(directory->text());
    QObject::connect(directory,&QLineEdit::textChanged,directory,&QWidget::setToolTip);
    auto *browse=new QPushButton("Browse…");
    auto *destination=new QHBoxLayout;
    destination->addWidget(directory,1);
    destination->addWidget(browse);
    form->addRow("Save to",destination);
    auto *countdown=new QSpinBox;
    countdown->setRange(0,30);
    countdown->setValue(original["recording"].toObject()["countdown_seconds"].toInt(3));
    countdown->setSuffix(" seconds");
    countdown->setAccessibleName("Countdown before recording");
    form->addRow("Countdown",countdown);
    auto *preview=new QLabel;
    wrapLabel(preview);
    preview->setObjectName("muted");
    form->addRow(preview);
    auto *message=new QLabel("Stop and save with hyprrecorder stop or your configured shortcut.");
    wrapLabel(message);
    form->addRow(message);
    // Status text can be arbitrarily long. Keep it scrollable, with the action
    // row always reachable even in a short tile or after an error.
    auto setMessage=[&](const QString &text) {
        message->setText(text);
        QTimer::singleShot(0,&window,[&]() { scroll->ensureWidgetVisible(message); });
    };
    auto *actions=new QHBoxLayout;
    auto *record=new QPushButton("Record");
    record->setObjectName("primary");
    record->setMinimumHeight(42);
    auto *openFolder=new QPushButton("Open folder");
    actions->addWidget(openFolder);
    actions->addWidget(record,1);
    outer->addLayout(actions);
    openFolder->hide();
    for(auto *combo : {mode,monitor,resolution,fps}) {
        combo->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
        combo->setMinimumContentsLength(6);
        combo->setSizePolicy(QSizePolicy::Expanding,QSizePolicy::Fixed);
        combo->setMinimumWidth(0);
    }
    // Keyboard navigation must reveal controls outside the current viewport.
    QObject::connect(&app,&QApplication::focusChanged,&window,[&](QWidget *,QWidget *now) {
        if(now && body->isAncestorOf(now)) {
            // Editable combo boxes focus their line edit; reveal the whole field.
            while(now->parentWidget()!=body)now=now->parentWidget();
            QTimer::singleShot(0,now,[scroll,body,now]() {
                // ensureWidgetVisible may use just an editor's cursor rectangle.
                const QRect field(now->mapTo(body,QPoint()),now->size());
                scroll->ensureVisible(field.center().x(),field.center().y(),
                    (field.width()+1)/2,(field.height()+1)/2+16);
            });
        }
    });
    QProcess worker,selector;
    // slurp reads optional rectangles from stdin before showing its overlay.
    // An open QProcess input pipe leaves it waiting forever for EOF.
    selector.setStandardInputFile(QProcess::nullDevice());
    selector.setChildProcessModifier([]() {
        prctl(PR_SET_PDEATHSIG,SIGTERM);
        if(getppid()==1)_exit(2);
    });
    QByteArray pending;
    QString saved,lastError;
    bool busy=false,selecting=false,selectionCancelled=false;
    QTimer selectionStart,selectionKill;
    selectionStart.setSingleShot(true);
    selectionKill.setSingleShot(true);
    QObject::connect(&selectionKill,&QTimer::timeout,[&]() {
        if(selecting && selector.state()!=QProcess::NotRunning)selector.kill();
    });
    auto resolutionValue=[&]() {
        int i=resolution->currentIndex();
        return i>=0 && resolution->currentText()==resolution->itemText(i)?resolution->itemData(i).toString():resolution->currentText();
    };
    auto updatePreview=[&]() {
        if(monitor->currentIndex()<0) {
            preview->setText("No monitor available");
            return;
        } auto m=monitors[monitor->currentIndex()].toObject();
        double w=m["width"].toDouble(),h=m["height"].toDouble();
        if(mode->currentIndex()==1) {
            if(region.isEmpty()) {
                preview->setText("Select a region before recording.");
                return;
            }double scale=m["scale"].toDouble(1);
            double left=(region["x"].toDouble()-m["x"].toDouble())*scale,top=(region["y"].toDouble()-m["y"].toDouble())*scale;
            w=int(std::floor(left+region["width"].toDouble()*scale)-std::ceil(left))&~1;
            h=int(std::floor(top+region["height"].toDouble()*scale)-std::ceil(top))&~1;
        } QString r=resolutionValue();
        int ow=int(w)&~1,oh=int(h)&~1;
        if(r!="native") {
            auto parts=r.split('x');
            if(parts.size()!=2) {
                preview->setText("Custom resolution: WIDTHxHEIGHT, even dimensions.");
                return;
            }ow=parts[0].toInt();
            oh=parts[1].toInt();
        } preview->setText(QString("Output %1 × %2%3. Aspect ratio is preserved with black padding. Unsupported geometry is checked before capture.").arg(ow).arg(oh).arg(ow>w||oh>h?" · upscaling":""));
    };
    auto show=[&]() {
        if(!busy && (smoke || !active())) {
            window.show();
            window.raise();
            window.activateWindow();
        }
    };
    auto reopen=[&]() {
        if(selecting) {
            selectionStart.stop();
            selectionCancelled=true;
            if(selector.state()!=QProcess::NotRunning) {
                // Wait for the overlay to exit before restoring the window.
                // Do not let repeated shortcut presses postpone the kill timer.
                if(!selectionKill.isActive()) {
                    selector.terminate();
                    selectionKill.start(500);
                }
                return;
            }
            selecting=false;
            busy=false;
            setMessage("Region selection cancelled.");
        }
        show();
    };
    QObject::connect(&server,&QLocalServer::newConnection,[&]() {
        while(auto *s=server.nextPendingConnection()) {
            QObject::connect(s,&QLocalSocket::readyRead,s,[&,s]() {
                if(!s->canReadLine()) {
                    if(s->bytesAvailable()>32)s->disconnectFromServer();
                    return;
                }QByteArray line=s->readLine(32);
                if(line=="show\n")reopen();
                if(line=="hide\n") {
                    if(!selecting) {
                        window.hide();
                        s->write("hidden\n");
                    }else s->write("busy\n");
                }s->disconnectFromServer();
            });
            QObject::connect(s,&QLocalSocket::disconnected,s,&QObject::deleteLater);
        }
    });
    QObject::connect(mode,&QComboBox::currentIndexChanged,[&]() {
        regionButton->setEnabled(!monitors.isEmpty() && mode->currentIndex()==1);
        updatePreview();
    });
    QObject::connect(monitor,&QComboBox::currentIndexChanged,[&]() {
        region= {
        };
        updatePreview();
    });
    QObject::connect(resolution,&QComboBox::currentTextChanged,[&]() {
        updatePreview();
    });
    QObject::connect(browse,&QPushButton::clicked,[&]() {
        auto path=QFileDialog::getExistingDirectory(&window,"Save recordings to");
        if(!path.isEmpty())directory->setText(path);
    });
    QObject::connect(openFolder,&QPushButton::clicked,[&]() {
        QString path=saved.isEmpty()?directory->text():QFileInfo(saved).absolutePath();
        if(path.startsWith("~/"))path=QDir::homePath()+path.mid(1);
        QProcess::startDetached("xdg-open", {
            path
        });
    });
    QObject::connect(regionButton,&QPushButton::clicked,[&]() {
        if(busy)return;
        busy=true;
        selecting=true;
        selectionCancelled=false;
        window.hide();
        selectionStart.start(200);
    });
    QObject::connect(&selectionStart,&QTimer::timeout,[&]() {
        selector.start("slurp", {
            "-f","%x %y %w %h"
        });
    });
    QObject::connect(&selector,&QProcess::errorOccurred,[&](QProcess::ProcessError e) {
        if(e==QProcess::FailedToStart) {
            selectionKill.stop();
            selecting=false;
            busy=false;
            setMessage("Region selection needs slurp. Install it or choose Screen.");
            show();
        }
    });
    QObject::connect(&selector,qOverload<int,QProcess::ExitStatus>(&QProcess::finished),[&](int code,QProcess::ExitStatus status) {
        selectionKill.stop();
        selecting=false;
        busy=false;
        const auto output=QString::fromUtf8(selector.readAllStandardOutput()).simplified();
        const auto error=QString::fromUtf8(selector.readAllStandardError()).trimmed().left(512);
        if(selectionCancelled) {
            setMessage("Region selection cancelled.");
        }else if(code==0 && status==QProcess::NormalExit) {
            setMessage("Region selection returned invalid coordinates. Please try again.");
            auto parts=output.split(' ');
            if(parts.size()==4) {
                bool ok=true;
                QStringList keys= {
                    "x","y","width","height"
                };
                QJsonObject next;
                for(int i=0;i<4;i++) {
                    bool valid;
                    int n=parts[i].toInt(&valid);
                    ok=ok&&valid;
                    next[keys[i]]=n;
                }if(ok && next["width"].toInt()>0 && next["height"].toInt()>0) {
                    region=next;
                    setMessage("Region selected. Review the output size, then Record.");
                }
            }
        }else if(status==QProcess::CrashExit) {
            setMessage("Region selector stopped unexpectedly. Please try again.");
        }else if(error.isEmpty() || error=="selection cancelled") {
            setMessage("Region selection cancelled.");
        }else setMessage("Region selection failed: "+error);
        updatePreview();
        show();
    });
    worker.setChildProcessModifier([]() {
        prctl(PR_SET_PDEATHSIG,SIGTERM);
        if(getppid()==1)_exit(2);
    });
    QObject::connect(record,&QPushButton::clicked,[&]() {
        if(busy || smoke)return;
        if(mode->currentIndex()==1 && region.isEmpty()) {
            regionButton->click();
            return;
        } QJsonObject settings=original,c=capture,v=video,a=audio;
        c["mode"]=mode->currentData().toString();
        c["monitor"]=monitor->currentData().toString();
        if(mode->currentIndex()==1)c["region"]=region;
        else c.remove("region");
        v["resolution"]=resolutionValue();
        v["fps"]=fps->currentText().toInt();
        v["cursor"]=cursor->isChecked();
        a["system"]=system->isChecked();
        a["microphone"]=mic->isChecked();
        settings["capture"]=c;
        settings["video"]=v;
        settings["audio"]=a;
        settings["output"]=QJsonObject {
            {
                "directory",directory->text()
            }
        };
        settings["recording"]=QJsonObject {
            {
                "countdown_seconds",countdown->value()
            }
        };
        QByteArray json=QJsonDocument(settings).toJson(QJsonDocument::Compact);
        char error[2048]= {
            0
        };
        if(save(json.constData(),error,sizeof(error))) {
            setMessage(QString::fromUtf8(error));
            return;
        } busy=true;
        saved.clear();
        lastError.clear();
        pending.clear();
        openFolder->hide();
        window.hide();
        QTimer::singleShot(300,[&]() {
            worker.start(exe, {
                "record","--config",config,"--countdown",QString::number(countdown->value())
            });
        });
    });
    QObject::connect(&worker,&QProcess::readyReadStandardOutput,[&]() {
        pending+=worker.readAllStandardOutput();
        int pos;
        while((pos=pending.indexOf('\n'))>=0) {
            auto event=QJsonDocument::fromJson(pending.left(pos)).object();
            pending.remove(0,pos+1);
            if(event["state"]=="saved")saved=event["path"].toString();
            if(event["state"]=="failed")lastError=event["error"].toString();
        }if(pending.size()>16384)pending.clear();
    });
    QObject::connect(&worker,&QProcess::readyReadStandardError,[&]() {
        worker.readAllStandardError();
    });
    QObject::connect(&worker,&QProcess::errorOccurred,[&](QProcess::ProcessError e) {
        if(e==QProcess::FailedToStart) {
            busy=false;
            setMessage("Cannot start recorder: "+worker.errorString());
            show();
        }
    });
    QObject::connect(&worker,qOverload<int,QProcess::ExitStatus>(&QProcess::finished),[&](int code,QProcess::ExitStatus status) {
        busy=false;
        if(code==0 && status==QProcess::NormalExit) {
            setMessage(saved.isEmpty()?"Recording cancelled before any frames were saved.":"Saved: "+saved);
            openFolder->setVisible(!saved.isEmpty());
        }else setMessage(lastError.isEmpty()?"Recording failed. A partial file may remain in the save directory.":lastError);
        show();
    });
    QObject::connect(&app,&QGuiApplication::lastWindowClosed,[&]() {
        if(!busy)app.quit();
    });
    if(startup["settings_error"].isString()) {
        setMessage(startup["settings_error"].toString());
        record->setEnabled(false);
    }
    if(monitors.isEmpty()) {
        setMessage("No eligible monitor was found. Check the display connection, then reopen Hyprrecorder.");
        record->setEnabled(false);
    }
    updatePreview();
    show();
    if(smoke)QTimer::singleShot(100,[&]() {
        // Exercise the real selection callbacks with a fake slurp on PATH.
        // This branch is available only in offscreen smoke mode, never normal UI.
        const QString regionCase=qEnvironmentVariable("HYPRRECORDER_SMOKE_REGION_CASE");
        if(!regionCase.isEmpty()) {
            const QJsonObject previousRegion{{"x",5},{"y",6},{"width",80},{"height",60}};
            region=previousRegion;
            mode->setCurrentIndex(1);
            auto verify=[&,regionCase,previousRegion]() {
                bool ok=!busy && !selecting && !selectionStart.isActive() &&
                    !selectionKill.isActive() && selector.state()==QProcess::NotRunning &&
                    window.isVisible() && record->isEnabled();
                if(regionCase=="success") {
                    ok=ok && region==QJsonObject{{"x",10},{"y",20},{"width",300},{"height",200}} &&
                        message->text().startsWith("Region selected.");
                }else {
                    ok=ok && region==previousRegion;
                    if(regionCase=="missing")ok=ok && message->text().contains("needs slurp");
                    else if(regionCase=="invalid")ok=ok && message->text().contains("invalid coordinates");
                    else if(regionCase=="crash")ok=ok && message->text().contains("unexpectedly");
                    else if(regionCase=="failure")ok=ok && message->text().contains("failed: test display failure");
                    else ok=ok && message->text()=="Region selection cancelled.";
                }
                // Reopening must still keep controls hidden while recording.
                busy=true;
                window.hide();
                reopen();
                ok=ok && !window.isVisible() && busy;
                busy=false;
                if(!ok)qCritical("Region smoke failed (%s): %s",qPrintable(regionCase),qPrintable(message->text()));
                else std::printf("Region smoke passed: %s\n",qPrintable(regionCase));
                app.exit(ok?0:6);
            };
            if(regionCase=="pending") {
                regionButton->click();
                reopen();
                QTimer::singleShot(350,&window,verify);
            }else {
                QObject::connect(&selector,qOverload<int,QProcess::ExitStatus>(&QProcess::finished),
                    &window,[&,verify](int,QProcess::ExitStatus) {
                        QTimer::singleShot(0,&window,verify);
                    });
                QObject::connect(&selector,&QProcess::errorOccurred,&window,[&,verify](QProcess::ProcessError e) {
                    if(e==QProcess::FailedToStart)QTimer::singleShot(0,&window,verify);
                });
                if(regionCase=="reopen" || regionCase=="stubborn") {
                    QObject::connect(&selector,&QProcess::readyReadStandardOutput,&window,[&]() {
                        if(selector.peek(6)=="ready\n")reopen();
                    });
                }
                regionButton->click();
            }
            QTimer::singleShot(3000,&window,[&]() {
                qCritical("Region smoke timed out with the window hidden");
                selector.kill();
                app.exit(7);
            });
            return;
        }
        bool ok=window.windowType()==Qt::Window && record->isEnabled() &&
            monitor->count()>0 && !monitor->currentData().toString().isEmpty();
        const QString originalDirectory=directory->text();
        const QString originalMessage=message->text();
        const QString originalMonitor=monitor->itemText(0);
        const QString originalResolution=resolution->currentText();
        const int originalMode=mode->currentIndex();
        // Include both sides of the reflow boundary, short tiles, and repeated
        // narrow/wide transitions. Long paths/status must never resize a tile.
        for(bool longText : {false,true}) {
            if(longText) {
                directory->setText("/home/example/"+QString(240,'x'));
                message->setText("Saved: "+directory->text()+"/recording.mp4");
                monitor->setItemText(0,QString(160,'M'));
                if(monitor->isHidden())form->insertRow(3,"Monitor",monitor);
                monitor->show();
                openFolder->show();
                mode->setCurrentIndex(1);
                resolution->setEditText("1600x900");
            }
            for(const QSize size : {QSize(300,220),QSize(360,640),QSize(559,360),
                                   QSize(560,360),QSize(960,540),QSize(1280,800),
                                   QSize(320,240),QSize(560,680)}) {
                window.resize(size);
                app.processEvents();
                app.processEvents();
                auto check=[&](bool passed,const char *what) {
                    if(!passed)qWarning("UI resize check failed: %s at %dx%d (long text: %d)",
                        what,size.width(),size.height(),longText);
                    ok=ok && passed;
                };
                check(window.size()==size,"compositor size respected");
                check(body->width()<=scroll->viewport()->width(),"no horizontal overflow");
                check(scroll->horizontalScrollBar()->maximum()==0,"no horizontal scrolling");
                check(form->rowWrapPolicy()==(size.width()<560?QFormLayout::WrapAllRows:
                    QFormLayout::DontWrapRows),"responsive labels");
                for(QWidget *control : QList<QWidget*>{mode,monitor,regionButton,resolution,fps,
                                                     cursor,system,mic,directory,browse,countdown}) {
                    if(!control->isVisible())continue;
                    const QRect bounds(control->mapTo(body,QPoint()),control->size());
                    check(bounds.left()>=0 && bounds.right()<body->width(),"control fits width");
                    if(!control->isEnabled())continue;
                    control->setFocus();
                    app.processEvents();
                    const QRect visible(control->mapTo(scroll->viewport(),QPoint()),control->size());
                    if(!scroll->viewport()->rect().contains(visible))
                        qWarning("Clipped %s (%s): y=%d height=%d viewport=%d",
                            control->metaObject()->className(),qPrintable(control->accessibleName()),
                            visible.y(),visible.height(),scroll->viewport()->height());
                    check(scroll->viewport()->rect().contains(visible),"focused control revealed");
                }
                record->setFocus();
                check(record->hasFocus(),"record keyboard focus");
                check(window.rect().contains(QRect(record->mapTo(&window,QPoint()),record->size())),
                    "record stays visible");
                check(!window.grab().isNull(),"window renders");
                check(directory->text()==(longText?"/home/example/"+QString(240,'x'):originalDirectory),
                    "resize preserves settings");
                check(resolution->currentText()==(longText?"1600x900":originalResolution),
                    "resize preserves custom resolution");
                check(mode->currentIndex()==(longText?1:originalMode),"resize preserves capture mode");
                const QString screenshots=qEnvironmentVariable("HYPRRECORDER_SMOKE_SCREENSHOTS");
                if(!screenshots.isEmpty()) {
                    scroll->verticalScrollBar()->setValue(0);
                    app.processEvents();
                    check(window.grab().save(screenshots+QString("/ui-%1x%2-%3.png")
                        .arg(size.width()).arg(size.height()).arg(longText)),"save smoke screenshot");
                }
            }
        }
        directory->setText(originalDirectory);
        message->setText(originalMessage);
        monitor->setItemText(0,originalMonitor);
        app.exit(ok?0:5);
    });
    return app.exec();
}

# プロジェクト概要

このプロジェクトは、ディスプレイの増減や解像度の変更を追跡するたのクレートを開発するプロジェクト。
プロジェクト名はdisplay-config-rsで、クレートの名前はdisplay-configである。

## 目的

このクレートは、RustのGUIアプリケーションの開発において、各ディスプレイにウィンドウを配置しておきたい、
というケースに対応するために開発が始まった。
例えば、画面上にちょっとしたペン書きをするOn-screen anotationアプリでは、ペン書きした絵を表示する
ウィンドウを各ディスプレイに配置する必要がある。そして、ディスプレイが増えたら増えた分そのウィンドウを増やす必要がある。

このような、ディスプレイ構成の変更に関心があるGUIアプリ開発を支援するためのクレートの開発が、このプロジェクトの目的である。

## 機能

### 関数群

ディスプレイを取得する関数群。返り値のディスプレイを表現する構造体は、後述の`Display`。

- `get_display`: 指定されたIDのディスプレイを取得する。
- `get_displays`: ディスプレイを全て取得する。

### 構造体`Display`

ディスプレイ情報を取得するための構造体で、以下のメソッドを提供する。

- `origin`: ディスプレイの場所の取得
- `size`: 解像度の取得
- `is_mirrored`: ミラーリングされているかどうかの取得
- `is_primary`: プライマリモニタかどうかの取得

### 構造体`DisplayObserver`

ディスプレイ構成の変更を監視するための構造体。後述の`Event`をラップした`MayBeDisplayAvailable`を配信する。
以下のメソッドを提供する。

- `new`: コンストラクタ、`DisplayObserver`のセットアップを行いインスタンス化する。
- `set_callback`: `DisplayObserver`のコールバックを設定する。
- `remove_callback`: `DisplayObserver`のコールバックを削除する。
- `run`: OSのイベントループか何かを使い、`DisplayObserver`を動かす。もし既にGUIアプリがあるなどの場合、これは使わない。
  例: tauriなど、別のGUIアプリでイベントループを動かしていて、そのイベントループで使いたい場合。

### 列挙型`MayBeDisplayAvailable`

イベントを配信する際、Removedイベントのようなディスプレイが無効になった場合、
`Display`構造体を配信することはできない。なぜなら、もうディスプレイがないため。

このため、イベント配信時は、まだディスプレイがある場合とない場合をこの列挙型で分け、
ディスプレイがある場合は`Display`を配信する。

### 列挙型`Event`

ディスプレイ構成変更イベントを表現する列挙型。

- `Added`: ディスプレイの追加
- `Removed`: ディスプレイの削除
- `Mirrored`: ディスプレイのミラーリング設定
- `UnMirrored`: ディスプレイのミラーリング解除
- `ResolutionChanged` ディスプレイの解像度変更

## アーキテクチャ

プラットフォーム別の実装は、`src/macos.rs`にmacOS、`src/windows.rs`にWindows向けの実装としてあり、
モジュール毎に分けられている。Linuxはいつか対応する予定であるが、現在は考慮しない。  
基本的にプラットフォーム固有の実装は前述のmacosモジュールとwindowsモジュールで行う。

`lib.rs`では、プラットフォーム固有の実装を
`PlatformDisplayObserver`や`PlatformDisplay`というようなPlatformをプリフィックスとして
インポートし、それをラップした`DisplayObserver`や`Display`を提供する。

なお、ユーザー側がプラットフォーム固有の実装を直接使う場合は、このPlatformをプリフィックスとするもの
ではなく、macosモジュール、windowsモジュールにある`MacOSDisplay`や`WindowsDisplay`といった、
OS名をプレフィックスとするものを直接使う。これは誤ってクロスプラットフォームではない
`PlatformDisplayObserver`などを使わないようにするためである。`MacOSDisplay`を使った場合、
Windowsではエラーになる。

### プラットフォーム固有の設計

#### Windows

WindowsではディスプレイのIDとしてデバイスパスを用いる。当初は`HMONITOR`を使う予定であったが、
設定が変わる毎に無効になる可能性があるため、デバイスパスを使うことになった。（ただ、未確認の項目があるので要調査。）

イベントの追跡には、見えないウィンドウを作り、そのイベントとして`WM_DISPLAYCHANGE`を受け取る。
ただ、これにはどのディスプレイの何の設定が変更されたのか不明なので、構造体`EventTracker`にて
設定変更の追跡を行う。（この構造体は外部に公開しない。）

#### macOS

macOSではIDとして`CGDirectDisplayID`を用いる。`MacOSDisplayId`として`CGDirectDisplayID`の
型エイリアスを提供する。`MacOSDisplay`はこのIDを所持し、諸々の情報にアクセスする。

イベントの追跡には、`CGDisplayRegisterReconfigurationCallback`を用いる。  
ただ、これによるイベント配信は、解像度の変更を追跡できないため、解像度の情報はキャッシュし自分で追跡を行う。
この実装は構造体`EventTracker`にある。（この構造体は外部に公開しない。）

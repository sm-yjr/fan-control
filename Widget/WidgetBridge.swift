import WidgetKit
@_cdecl("fan_control_reload_widgets")
public func fanControlReloadWidgets() {
    WidgetCenter.shared.reloadTimelines(ofKind: "FanControlReadings")
}

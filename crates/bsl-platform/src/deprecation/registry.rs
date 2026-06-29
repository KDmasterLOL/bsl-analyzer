// allow: SIZE_OK — manual deprecation fact table is intentionally co-located for duplicate-key validation.
use super::types::{
    CompatibilityBucket, DeprecationEntry, DisplayKind, ElementKind, LifecycleGroup, OwnerType,
    Replacement,
};
use CompatibilityBucket::{
    Any, CompatibilityMode8_3_10, CompatibilityMode8_3_12, CompatibilityMode8_3_14,
    CompatibilityMode8_3_17, CompatibilityMode8_3_6,
};
use LifecycleGroup::{
    ApplicationInterface, ChartPresentation, DateTime, ErrorProcessing, EventLog, ManagedForm,
    StringSearch, UserNotification,
};

const CHART: OwnerType = OwnerType { ru: "Диаграмма", en: "Chart" };
const GANTT_CHART: OwnerType = OwnerType { ru: "ДиаграммаГанта", en: "GanttChart" };
const PIVOT_CHART: OwnerType =
    OwnerType { ru: "СводнаяДиаграмма", en: "PivotChart" };
const CHART_PLOT_AREA: OwnerType =
    OwnerType { ru: "ОбластьПостроенияДиаграммы", en: "ChartPlotArea" };
const CHART_LABELS_ORIENTATION: OwnerType =
    OwnerType { ru: "ОриентацияМетокДиаграммы", en: "" };
const CHILD_FORM_ITEMS_GROUP: OwnerType = OwnerType {
    ru: "ГруппировкаПодчиненныхЭлементовФормы",
    en: "ChildFormItemsGroup",
};
const HTTP_CONNECTION: OwnerType =
    OwnerType { ru: "HTTPСоединение", en: "HTTPConnection" };
const INTERNET_PROXY: OwnerType =
    OwnerType { ru: "ИнтернетПрокси", en: "InternetProxy" };

macro_rules! replacement {
    ($ru:literal, $en:literal) => {
        Some(Replacement { ru: $ru, en: $en })
    };
}

macro_rules! entry {
    ($ru:literal, $en:literal, $element_kind:expr, $owner:expr, $group:expr, $replacement:expr, $compatibility:expr, $display:expr) => {
        DeprecationEntry {
            ru: $ru,
            en: $en,
            element_kind: $element_kind,
            owner: $owner,
            group: $group,
            replacement: $replacement,
            compatibility: $compatibility,
            display: $display,
        }
    };
}

macro_rules! global_function {
    ($ru:literal, $en:literal, $group:expr, $replacement:expr, $compatibility:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::GlobalMethod,
            None,
            $group,
            $replacement,
            $compatibility,
            DisplayKind::Function
        )
    };
}

macro_rules! deprecated_method {
    ($ru:literal, $en:literal, $group:expr, $replacement:expr, $compatibility:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::GlobalMethod,
            None,
            $group,
            $replacement,
            $compatibility,
            DisplayKind::Method
        )
    };
}

macro_rules! global_method {
    ($ru:literal, $en:literal, $group:expr, $replacement:expr, $compatibility:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::GlobalMethod,
            None,
            $group,
            $replacement,
            $compatibility,
            DisplayKind::GlobalMethod
        )
    };
}

macro_rules! type_entry {
    ($ru:literal, $en:literal, $group:expr, $replacement:expr, $compatibility:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::Type,
            None,
            $group,
            $replacement,
            $compatibility,
            DisplayKind::Type
        )
    };
}

macro_rules! attribute {
    ($owner:expr, $ru:literal, $en:literal, $replacement:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::Attribute,
            Some($owner),
            ChartPresentation,
            $replacement,
            CompatibilityMode8_3_12,
            DisplayKind::Attribute
        )
    };
}

macro_rules! method {
    ($owner:expr, $ru:literal, $en:literal, $replacement:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::Method,
            Some($owner),
            ChartPresentation,
            $replacement,
            CompatibilityMode8_3_12,
            DisplayKind::Method
        )
    };
}

macro_rules! property {
    ($owner:expr, $ru:literal, $en:literal, $replacement:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::Property,
            Some($owner),
            ApplicationInterface,
            $replacement,
            CompatibilityMode8_3_17,
            DisplayKind::Property
        )
    };
}

macro_rules! enum_name {
    ($ru:literal, $en:literal, $owner:expr, $group:expr, $replacement:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::EnumName,
            $owner,
            $group,
            $replacement,
            CompatibilityMode8_3_12,
            DisplayKind::EnumName
        )
    };
}

macro_rules! enum_value {
    ($ru:literal, $en:literal, $owner:expr, $group:expr, $replacement:expr) => {
        entry!(
            $ru,
            $en,
            ElementKind::EnumValue,
            Some($owner),
            $group,
            $replacement,
            CompatibilityMode8_3_12,
            DisplayKind::EnumValue
        )
    };
}

pub const ENTRIES: &[DeprecationEntry] = &[
    global_function!(
        "ТекущаяДата",
        "CurrentDate",
        DateTime,
        replacement!("ТекущаяДатаСеанса", "CurrentSessionDate"),
        Any
    ),
    global_function!("Найти", "Find", StringSearch, replacement!("СтрНайти", "StrFind"), CompatibilityMode8_3_6),
    global_function!(
        "Сообщить",
        "Message",
        UserNotification,
        replacement!("ОбщегоНазначения.СообщитьПользователю", "CommonUse.MessageToUser"),
        Any
    ),
    type_entry!(
        "УправляемаяФорма",
        "ManagedForm",
        ManagedForm,
        replacement!("ФормаКлиентскогоПриложения", "ClientApplicationForm"),
        CompatibilityMode8_3_14
    ),
    deprecated_method!(
        "УстановитьКраткийЗаголовокПриложения",
        "SetShortApplicationCaption",
        ApplicationInterface,
        replacement!("КлиентскоеПриложение.УстановитьКраткийЗаголовок", "ClientApplication.SetShortCaption"),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "ПолучитьКраткийЗаголовокПриложения",
        "GetShortApplicationCaption",
        ApplicationInterface,
        replacement!("КлиентскоеПриложение.ПолучитьКраткийЗаголовок", "ClientApplication.GetShortCaption"),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "УстановитьЗаголовокКлиентскогоПриложения",
        "SetClientApplicationCaption",
        ApplicationInterface,
        replacement!("КлиентскоеПриложение.УстановитьЗаголовок", "ClientApplication.SetCaption"),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "ПолучитьЗаголовокКлиентскогоПриложения",
        "GetClientApplicationCaption",
        ApplicationInterface,
        replacement!("КлиентскоеПриложение.ПолучитьЗаголовок", "ClientApplication.GetCaption"),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "ТекущийВариантОсновногоШрифтаКлиентскогоПриложения",
        "ClientApplicationBaseFontCurrentVariant",
        ApplicationInterface,
        replacement!(
            "КлиентскоеПриложение.ТекущийВариантОсновногоШрифта",
            "ClientApplication.CurrentBaseFontVariant"
        ),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "ТекущийВариантИнтерфейсаКлиентскогоПриложения",
        "ClientApplicationInterfaceCurrentVariant",
        ApplicationInterface,
        replacement!(
            "КлиентскоеПриложение.ТекущийВариантИнтерфейса",
            "ClientApplication.CurrentInterfaceVariant"
        ),
        CompatibilityMode8_3_10
    ),
    deprecated_method!(
        "КраткоеПредставлениеОшибки",
        "BriefErrorRepresentation",
        ErrorProcessing,
        replacement!(
            "МенеджерОбработкиОшибок.КраткоеПредставлениеОшибки",
            "ErrorProcessingManager.BriefErrorRepresentation"
        ),
        CompatibilityMode8_3_17
    ),
    deprecated_method!(
        "ПодробноеПредставлениеОшибки",
        "DetailedErrorRepresentation",
        ErrorProcessing,
        replacement!(
            "МенеджерОбработкиОшибок.ПодробноеПредставлениеОшибки",
            "ErrorProcessingManager.DetailedErrorRepresentation"
        ),
        CompatibilityMode8_3_17
    ),
    deprecated_method!(
        "ПоказатьИнформациюОбОшибке",
        "ShowErrorInformation",
        ErrorProcessing,
        replacement!(
            "МенеджерОбработкиОшибок.ПоказатьИнформациюОбОшибке",
            "ErrorProcessingManager.ShowErrorInformation"
        ),
        CompatibilityMode8_3_17
    ),
    deprecated_method!(
        "ПолучитьФорму",
        "GetForm",
        ManagedForm,
        replacement!("ОткрытьФорму", "OpenForm"),
        CompatibilityMode8_3_17
    ),
    entry!(
        "Получить",
        "Get",
        ElementKind::Method,
        Some(HTTP_CONNECTION),
        ApplicationInterface,
        replacement!("ПолучитьАсинх", "GetAsync"),
        CompatibilityMode8_3_17,
        DisplayKind::Method
    ),
    property!(INTERNET_PROXY, "Пароль", "Password", replacement!("Пароль", "Password")),
    property!(INTERNET_PROXY, "Пользователь", "User", replacement!("Пользователь", "User")),
    attribute!(CHART_PLOT_AREA, "ОтображатьШкалу", "ShowScale", replacement!("ОтображатьШкалы", "ShowScales")),
    attribute!(CHART_PLOT_AREA, "ЛинииШкалы", "", replacement!("ЛинииШкал", "")),
    attribute!(CHART_PLOT_AREA, "ЦветШкалы", "", replacement!("ЦветШкал", "")),
    attribute!(
        CHART_PLOT_AREA,
        "ОтображатьПодписиШкалыСерий",
        "ShowSeriesScaleLabels",
        replacement!("ШкалаСерий.ПоложениеПодписейШкалы", "SeriesScale.ScaleLabelLocation")
    ),
    attribute!(
        CHART_PLOT_AREA,
        "ОтображатьПодписиШкалыТочек",
        "ShowPointsScaleLabels",
        replacement!("ШкалаТочек.ПоложениеПодписейШкалы", "PointsScale.ScaleLabelLocation")
    ),
    attribute!(
        CHART_PLOT_AREA,
        "ОтображатьПодписиШкалыЗначений",
        "ShowValuesScaleLabels",
        replacement!("ШкалаЗначений.ПоложениеПодписейШкалы", "ValuesScale.ScaleLabelLocation")
    ),
    attribute!(
        CHART_PLOT_AREA,
        "ОтображатьЛинииЗначенийШкалы",
        "ShowScaleValueLines",
        replacement!("ШкалаЗначений.ОтображениеЛинийСетки", "ValuesScale.GridLinesShowMode")
    ),
    attribute!(
        CHART_PLOT_AREA,
        "ФорматШкалыЗначений",
        "ValueScaleFormat",
        replacement!("ШкалаЗначений.ФорматПодписей", "ValuesScale.LabelFormat")
    ),
    attribute!(
        CHART_PLOT_AREA,
        "ОриентацияМеток",
        "LabelsOrientation",
        replacement!("ШкалаТочек.ОриентацияПодписей", "PointsScale.LabelOrientation")
    ),
    attribute!(
        CHART,
        "ОтображатьЛегенду",
        "ShowLegend",
        replacement!(
            "одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы",
            "one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea"
        )
    ),
    attribute!(
        CHART,
        "ОтображатьЗаголовок",
        "ShowTitle",
        replacement!(
            "одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы",
            "one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea"
        )
    ),
    attribute!(
        CHART,
        "ПалитраЦветов",
        "ColorPalette",
        replacement!("ОписаниеПалитрыЦветов.ПалитраЦветов", "ColorPaletteDescription.ColorPalette")
    ),
    attribute!(
        CHART,
        "ЦветНачалаГрадиентнойПалитры",
        "GradientPaletteStartColor",
        replacement!(
            "ОписаниеПалитрыЦветов.ЦветНачалаГрадиентнойПалитры",
            "ColorPaletteDescription.GradientPaletteStartColor"
        )
    ),
    attribute!(
        CHART,
        "ЦветКонцаГрадиентнойПалитры",
        "GradientPaletteEndColor",
        replacement!(
            "ОписаниеПалитрыЦветов.ЦветКонцаГрадиентнойПалитры",
            "ColorPaletteDescription.GradientPaletteEndColor"
        )
    ),
    attribute!(
        CHART,
        "МаксимальноеКоличествоЦветовГрадиентнойПалитры",
        "GradientPaletteMaxColors",
        replacement!(
            "ОписаниеПалитрыЦветов.МаксимальноеКоличествоЦветовГрадиентнойПалитры",
            "ColorPaletteDescription.GradientPaletteMaxColors"
        )
    ),
    attribute!(GANTT_CHART, "ОтображатьЛегенду", "ShowLegend", replacement!(
        "одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы",
        "one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea"
    )),
    attribute!(GANTT_CHART, "ОтображатьЗаголовок", "ShowTitle", replacement!(
        "одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы",
        "one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea"
    )),
    attribute!(GANTT_CHART, "ПалитраЦветов", "ColorPalette", replacement!("ОписаниеПалитрыЦветов.ПалитраЦветов", "ColorPaletteDescription.ColorPalette")),
    attribute!(GANTT_CHART, "ЦветНачалаГрадиентнойПалитры", "GradientPaletteStartColor", replacement!(
        "ОписаниеПалитрыЦветов.ЦветНачалаГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteStartColor"
    )),
    attribute!(GANTT_CHART, "ЦветКонцаГрадиентнойПалитры", "GradientPaletteEndColor", replacement!(
        "ОписаниеПалитрыЦветов.ЦветКонцаГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteEndColor"
    )),
    attribute!(GANTT_CHART, "МаксимальноеКоличествоЦветовГрадиентнойПалитры", "GradientPaletteMaxColors", replacement!(
        "ОписаниеПалитрыЦветов.МаксимальноеКоличествоЦветовГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteMaxColors"
    )),
    attribute!(PIVOT_CHART, "ОтображатьЛегенду", "ShowLegend", replacement!(
        "одно из свойств ОбластьЛегендыДиаграммы, ОбластьЛегендыДиаграммыГанта или ОбластьЛегендыСводнойДиаграммы",
        "one of the properties of ChartLegendArea, GanttChartLegendArea or PivotChartLegendArea"
    )),
    attribute!(PIVOT_CHART, "ОтображатьЗаголовок", "ShowTitle", replacement!(
        "одно из свойств ОбластьЗаголовкаДиаграммы, ОбластьЗаголовкаДиаграммыГанта или ОбластьЗаголовкаСводнойДиаграммы",
        "one of the properties of ChartTitleArea, GanttChartTitleArea or PivotChartTitleArea"
    )),
    attribute!(PIVOT_CHART, "ПалитраЦветов", "ColorPalette", replacement!("ОписаниеПалитрыЦветов.ПалитраЦветов", "ColorPaletteDescription.ColorPalette")),
    attribute!(PIVOT_CHART, "ЦветНачалаГрадиентнойПалитры", "GradientPaletteStartColor", replacement!(
        "ОписаниеПалитрыЦветов.ЦветНачалаГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteStartColor"
    )),
    attribute!(PIVOT_CHART, "ЦветКонцаГрадиентнойПалитры", "GradientPaletteEndColor", replacement!(
        "ОписаниеПалитрыЦветов.ЦветКонцаГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteEndColor"
    )),
    attribute!(PIVOT_CHART, "МаксимальноеКоличествоЦветовГрадиентнойПалитры", "GradientPaletteMaxColors", replacement!(
        "ОписаниеПалитрыЦветов.МаксимальноеКоличествоЦветовГрадиентнойПалитры",
        "ColorPaletteDescription.GradientPaletteMaxColors"
    )),
    method!(
        CHART,
        "ПолучитьПалитру",
        "GetPalette",
        replacement!("ОписаниеПалитрыЦветов.ПолучитьПалитру", "ColorPaletteDescription.GetPalette")
    ),
    method!(
        CHART,
        "УстановитьПалитру",
        "SetPalette",
        replacement!("ОписаниеПалитрыЦветов.УстановитьПалитру", "ColorPaletteDescription.SetPalette")
    ),
    method!(
        GANTT_CHART,
        "ПолучитьПалитру",
        "GetPalette",
        replacement!("ОписаниеПалитрыЦветов.ПолучитьПалитру", "ColorPaletteDescription.GetPalette")
    ),
    method!(
        GANTT_CHART,
        "УстановитьПалитру",
        "SetPalette",
        replacement!("ОписаниеПалитрыЦветов.УстановитьПалитру", "ColorPaletteDescription.SetPalette")
    ),
    method!(
        PIVOT_CHART,
        "ПолучитьПалитру",
        "GetPalette",
        replacement!("ОписаниеПалитрыЦветов.ПолучитьПалитру", "ColorPaletteDescription.GetPalette")
    ),
    method!(
        PIVOT_CHART,
        "УстановитьПалитру",
        "SetPalette",
        replacement!("ОписаниеПалитрыЦветов.УстановитьПалитру", "ColorPaletteDescription.SetPalette")
    ),
    global_method!("ОчиститьЖурналРегистрации", "ClearEventLog", EventLog, None, CompatibilityMode8_3_12),
    enum_name!(
        "ОриентацияМетокДиаграммы",
        "",
        None,
        ChartPresentation,
        replacement!("ОриентацияПодписейДиаграммы", "")
    ),
    enum_name!("Авто", "", Some(CHART_LABELS_ORIENTATION), ChartPresentation, None),
    enum_value!(
        "Горизонтальная",
        "Horizontal",
        CHILD_FORM_ITEMS_GROUP,
        ManagedForm,
        replacement!("ГоризонтальнаяВсегда", "AlwaysHorizontal")
    ),
];

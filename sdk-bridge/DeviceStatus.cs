using DYMO.LabelAPI;
using DYMO.LabelAPI.Windows;
using DYMO.LabelAPI.Windows.Printers;

// The same read-only legacy status entry point used by the vendor's 420P
// communication class. No arbitrary/raw commands or device settings are sent.
internal sealed record DeviceStatusResult(bool Connected, bool StatusAvailable,
    int? BatteryPercent = null, bool? Charging = null, int? TapeWidthMm = null,
    string? Message = null)
{
    public string[] Issues { get; init; } = [];
}

internal static class DeviceStatus
{
    internal const string PrinterName = "DYMO LabelManager 420P";
    internal static DeviceStatusResult Disconnected() => new(false, false);

    internal static DeviceStatusResult Read()
    {
        if (!PrintingSupportGateway.IsPrinterConnected(PrinterName)) return Disconnected();
        try { return Decode(PrintingSupportGateway.GetTapePrinterStatus(PrinterName)); }
        catch (Exception) { return new(true, false, Message: "Printer details are unavailable. Try refreshing the connection."); }
    }

    internal static DeviceStatusResult Decode(byte[] bytes)
    {
        // Reject incomplete data rather than converting a missing reply to 0%.
        if (bytes is not { Length: 8 }) return new(true, false, Message: "The printer did not return a complete status reply.");
        var status = PnpStatus.Parse(bytes);
        var issues = new List<string>();
        switch (status.PrintError)
        {
            case PnpStatus.Enums.PrintError.GeneralError: issues.Add("Printer error. Check the tape exit and the printer display."); break;
            case PnpStatus.Enums.PrintError.NoPowerToPrint: issues.Add("Battery power is too low to print. Charge the printer."); break;
        }
        // The SDK's single PrintError gives low power precedence over cassette
        // absence. Preserve both independent flags and never show stale width.
        var cassettePresent = (bytes[0] & 0x40) != 0;
        if (!cassettePresent) issues.Add("No cassette detected. Insert a D1 cassette.");
        switch (status.BatteryError)
        {
            case PnpStatus.Enums.BatteryError.PowerError: issues.Add("Battery power error. Check the battery and charger."); break;
            case PnpStatus.Enums.BatteryError.NoBattery: issues.Add("No battery detected."); break;
            case PnpStatus.Enums.BatteryError.NoUsbCurrent: issues.Add("The printer reports insufficient USB charging current."); break;
        }
        if (status.BatteryStatus == PnpStatus.Enums.BatteryStatus.ChargeError && status.BatteryError == PnpStatus.Enums.BatteryError.None)
            issues.Add("The printer reports a charging error.");
        var hasBattery = status.BatteryError != PnpStatus.Enums.BatteryError.NoBattery;
        int? percentage = hasBattery && status.BatteryChargedPercentage is >= 0 and <= 100 ? status.BatteryChargedPercentage : null;
        bool? charging = hasBattery && status.BatteryStatus != PnpStatus.Enums.BatteryStatus.ChargeError
            ? status.BatteryStatus == PnpStatus.Enums.BatteryStatus.Charging : null;
        int? width = !cassettePresent ? null : status.TapeWidth switch {
            ETapeWidth.tw6mm => 6, ETapeWidth.tw9mm => 9, ETapeWidth.tw12mm => 12, ETapeWidth.tw19mm => 19, _ => null
        };
        return new(true, true, percentage, charging, width) { Issues = issues.ToArray() };
    }
}

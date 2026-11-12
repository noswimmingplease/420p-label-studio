using System.Globalization;
using System.Reflection;
using DYMO.LabelAPI;
using DYMO.LabelAPI.Interfaces;
using DymoSDK.Implementations;
using DymoSDK.Interfaces;
using ModelLabelObject = DYMO.LabelAPI.Interfaces.ILabelObject;

// Object positions are in millimetres relative to the SDK's printable-area origin.
// Value is text/data for text, address, barcode or QR; for date/time it is a
// required fixed ISO value captured by the client before preview. Variant is a
// barcode format, LineType, ShapeType or Date/Time/DateAndTime name.
internal sealed record FeatureObject(
    string Kind, string Name, string? Value,
    double Xmm, double Ymm, double WidthMm, double HeightMm,
    string? Variant = null, int StartAt = 1, int Increment = 1, int TextWidth = 1,
    bool LeadingZeros = false);

internal sealed record FeatureObjectLayout(
    string Name, string Kind, double Xmm, double Ymm, double WidthMm, double HeightMm,
    bool ResizedBySdk);

internal static class FeatureObjects
{
    private const double MillimetresPerInch = 25.4;
    private const double ToleranceMm = 0.15;

    // Adds objects to an already loaded label and performs a non-printing render.
    // Caller must show returned SDK-resolved geometry before allowing a print: some
    // DYMO objects auto-resize and are not guaranteed to retain the requested size.
    // Defer overlap checks only when the caller will apply and validate a
    // complete explicit layout on this same fresh model before any print.
    internal static IReadOnlyList<FeatureObjectLayout> Add(IDymoLabel label, IReadOnlyList<FeatureObject> objects, bool deferOverlapValidation = false, string? emptyPrimaryText = null)
    {
        ArgumentNullException.ThrowIfNull(label);
        ArgumentNullException.ThrowIfNull(objects);
        if (objects.Count > 50) throw new InvalidOperationException("A label can contain at most 50 additional objects.");
        if (objects.Count == 0) return Array.Empty<FeatureObjectLayout>();

        var field = typeof(DymoLabel).GetField("_instanceLabel", BindingFlags.Instance | BindingFlags.NonPublic);
        var model = field?.GetValue(label) as ILabel<ILayoutManager>
            ?? throw new InvalidOperationException("The DYMO SDK label model is unavailable; objects cannot be added safely.");
        if (label.GetPreviewLabel() is not { Length: > 0 }) throw new InvalidOperationException("DYMO could not render the existing label.");
        var bounds = model.LayoutBounds;
        var areaWidth = bounds.Size.Width * MillimetresPerInch;
        var areaHeight = bounds.Size.Height * MillimetresPerInch;
        if (!double.IsFinite(areaWidth) || !double.IsFinite(areaHeight) || areaWidth <= 0 || areaHeight <= 0)
            throw new InvalidOperationException("The DYMO SDK did not provide valid printable bounds.");

        var names = new HashSet<string>(model.LayoutManager.LabelObjects.Select(o => o.Name), StringComparer.OrdinalIgnoreCase);
        foreach (var spec in objects)
        {
            if (spec is null || string.IsNullOrWhiteSpace(spec.Kind)) throw new InvalidOperationException("Object type is missing.");
            if (string.IsNullOrEmpty(spec.Name) || spec.Name.Length > 64 || !char.IsAsciiLetter(spec.Name[0]) ||
                spec.Name.Any(ch => !char.IsAsciiLetterOrDigit(ch) && ch is not '_' and not '-'))
                throw new InvalidOperationException("Object names must start with a letter and contain only letters, numbers, hyphens or underscores.");
            if (!names.Add(spec.Name)) throw new InvalidOperationException($"Object name '{spec.Name}' is already used.");
            if (!double.IsFinite(spec.Xmm) || !double.IsFinite(spec.Ymm) || !double.IsFinite(spec.WidthMm) || !double.IsFinite(spec.HeightMm) ||
                spec.Xmm < 0 || spec.Ymm < 0 || spec.WidthMm <= 0 || spec.HeightMm <= 0 || spec.Xmm + spec.WidthMm > 2000 ||
                spec.Ymm + spec.HeightMm > areaHeight + ToleranceMm)
                throw new InvalidOperationException($"Object '{spec.Name}' is outside the printable tape height or has invalid dimensions.");
            ValidateContent(spec);
        }

        // Existing text or image objects may expand when the SDK renders them.
        // Start after their resolved right edge so newly added objects do not
        // cover them. Additional objects are resolved sequentially for the same
        // reason: barcode widths in particular can grow substantially.
        var existing = model.LayoutManager.LabelObjects.ToList();
        double cursorMm = existing.Count == 0 ? 0 : existing.Max(o =>
            (o.Layout.Rect.Location.X + o.Layout.Rect.Size.Width - bounds.Location.X) * MillimetresPerInch);
        cursorMm = Math.Max(0, cursorMm);
        if (!double.IsFinite(cursorMm) || cursorMm > 2000) throw new InvalidOperationException("The existing label geometry is invalid or too long.");
        var firstAvailableMm = cursorMm;
        var created = new List<(FeatureObject Spec, ModelLabelObject Object)>();
        foreach (var spec in objects)
        {
            var placementMm = Math.Max(cursorMm, firstAvailableMm + spec.Xmm);
            if (!double.IsFinite(placementMm) || placementMm + spec.WidthMm > 2000)
                throw new InvalidOperationException($"Object '{spec.Name}' would make the label too long.");
            var currentBounds = model.LayoutBounds;
            var kind = spec.Kind.ToLowerInvariant();
            ModelLabelObject obj = kind switch
            {
                "text" => model.LayoutManager.AddLabelObject<ITextObject>(spec.Name),
                "address" => model.LayoutManager.AddLabelObject<IAddressObject>(spec.Name),
                "barcode" => model.LayoutManager.AddLabelObject<IBarcodeObject>(spec.Name),
                "qr" => model.LayoutManager.AddLabelObject<IQRCodeObject>(spec.Name),
                "line" => model.LayoutManager.AddLabelObject<ILineObject>(spec.Name),
                "shape" => model.LayoutManager.AddLabelObject<IShapeObject>(spec.Name),
                "date" => model.LayoutManager.AddLabelObject<IDateTimeObject>(spec.Name),
                "counter" => model.LayoutManager.AddLabelObject<ICounterObject>(spec.Name),
                _ => throw new InvalidOperationException($"Unsupported object type '{spec.Kind}'.")
            };
            obj.Layout.ResetRect(
                new DYMOPoint(currentBounds.Location.X + (float)(placementMm / MillimetresPerInch), currentBounds.Location.Y + (float)(spec.Ymm / MillimetresPerInch)),
                new DYMOSize((float)(spec.WidthMm / MillimetresPerInch), (float)(spec.HeightMm / MillimetresPerInch)));

            switch (kind)
            {
                case "text":
                case "address":
                    var sdkObject = label.GetLabelObject(spec.Name) ?? throw new InvalidOperationException($"DYMO did not expose object '{spec.Name}'.");
                    if (!label.UpdateLabelObject(sdkObject, spec.Value!)) throw new InvalidOperationException($"DYMO could not set the text of '{spec.Name}'.");
                    break;
                case "barcode":
                    if (obj is not BarcodeObject barcode) throw new InvalidOperationException("DYMO did not create a barcode object.");
                    barcode.BarcodeType = BarcodeType(spec.Variant);
                    // The SDK flags Code 39 as too small at Small and Medium.
                    barcode.BarcodeSize = barcode.BarcodeType == EBarcodeType.Code39 ? EBarcodeSize.Large : EBarcodeSize.Small;
                    barcode.TextPosition = EHRTPosition.None;
                    barcode.Data.Text = spec.Value!;
                    break;
                case "qr":
                    if (obj is not QRCodeObject qr) throw new InvalidOperationException("DYMO did not create a QR object.");
                    qr.GenerateDataHolder(EQRCodeType.QRCodeText);
                    qr.Data.Text = spec.Value!;
                    break;
                case "line":
                    ((ILineObject)obj).LineType = Enum.Parse<ELineType>(spec.Variant ?? nameof(ELineType.Horizontal));
                    break;
                case "shape":
                    ((IShapeObject)obj).ShapeType = Enum.Parse<EShapeType>(spec.Variant ?? nameof(EShapeType.Rectangle));
                    break;
                case "date":
                    var date = (IDateTimeObject)obj;
                    var capturedDateTime = ParseDateTime(spec, out var timeFormat);
                    date.TimeFormat = timeFormat;
                    date.Format = EDateFormat.ShortSystemDate;
                    date.Use24HourTime = true;
                    date.IsStatic = true;
                    date.StaticDateTime = capturedDateTime;
                    break;
                case "counter":
                    var counter = (ICounterObject)obj;
                    counter.StartAt = spec.StartAt;
                    counter.Increment = spec.Increment;
                    counter.TextWidth = spec.TextWidth;
                    counter.PrintLeadingZeros = spec.LeadingZeros;
                    break;
            }
            created.Add((spec, obj));
            if (label.GetPreviewLabel() is not { Length: > 0 }) throw new InvalidOperationException($"DYMO could not render object '{spec.Name}'.");
            var interimBounds = model.LayoutBounds;
            cursorMm = model.LayoutManager.LabelObjects.Max(o =>
                (o.Layout.Rect.Location.X + o.Layout.Rect.Size.Width - interimBounds.Location.X) * MillimetresPerInch);
            if (!double.IsFinite(cursorMm) || cursorMm > 2000) throw new InvalidOperationException("DYMO expanded the label beyond the supported length.");
        }

        // DYMO auto-expands some objects and the label while rendering. Check the
        // resolved bounds rather than claiming the input geometry stayed unchanged.
        if (label.GetPreviewLabel() is not { Length: > 0 }) throw new InvalidOperationException("DYMO could not render the objects.");
        if (!model.LayoutManager.IsValidRender) throw new InvalidOperationException("The label cannot be rendered with these objects.");
        var finalBounds = model.LayoutBounds;
        var allObjects = model.LayoutManager.LabelObjects.ToList();
        var output = new List<FeatureObjectLayout>(created.Count);
        foreach (var (spec, obj) in created)
        {
            if (!obj.IsValidRender || (obj is BarcodeObject barcode && barcode.BarcodeIsTooSmall))
                throw new InvalidOperationException($"Object '{spec.Name}' is invalid or too small to render.");
            var rect = obj.Layout.Rect;
            var x = (rect.Location.X - finalBounds.Location.X) * MillimetresPerInch;
            var y = (rect.Location.Y - finalBounds.Location.Y) * MillimetresPerInch;
            var width = rect.Size.Width * MillimetresPerInch;
            var height = rect.Size.Height * MillimetresPerInch;
            var finalWidth = finalBounds.Size.Width * MillimetresPerInch;
            var finalHeight = finalBounds.Size.Height * MillimetresPerInch;
            if (!double.IsFinite(x) || !double.IsFinite(y) || !double.IsFinite(width) || !double.IsFinite(height) ||
                x < -ToleranceMm || y < -ToleranceMm || width <= 0 || height <= 0 ||
                x + width > finalWidth + ToleranceMm || y + height > finalHeight + ToleranceMm)
                throw new InvalidOperationException($"DYMO placed object '{spec.Name}' outside the printable canvas.");
            foreach (var other in allObjects.Where(o => !ReferenceEquals(o, obj)))
            {
                if (emptyPrimaryText is not null && string.Equals(other.Name, emptyPrimaryText, StringComparison.OrdinalIgnoreCase)) continue;
                var otherRect = other.Layout.Rect;
                var overlapX = Math.Min(rect.Location.X + rect.Size.Width, otherRect.Location.X + otherRect.Size.Width) - Math.Max(rect.Location.X, otherRect.Location.X);
                var overlapY = Math.Min(rect.Location.Y + rect.Size.Height, otherRect.Location.Y + otherRect.Size.Height) - Math.Max(rect.Location.Y, otherRect.Location.Y);
                if (!deferOverlapValidation && overlapX * MillimetresPerInch > ToleranceMm && overlapY * MillimetresPerInch > ToleranceMm)
                    throw new InvalidOperationException($"Object '{spec.Name}' overlaps '{other.Name}'; overlapping objects are not yet supported.");
            }
            var changed = Math.Abs(x - spec.Xmm) > ToleranceMm || Math.Abs(y - spec.Ymm) > ToleranceMm ||
                Math.Abs(width - spec.WidthMm) > ToleranceMm || Math.Abs(height - spec.HeightMm) > ToleranceMm;
            output.Add(new FeatureObjectLayout(spec.Name, spec.Kind, x, y, width, height, changed));
        }
        return output;
    }

    private static void ValidateContent(FeatureObject spec)
    {
        switch (spec.Kind.ToLowerInvariant())
        {
            case "text":
            case "address":
                if (string.IsNullOrWhiteSpace(spec.Value) || spec.Value.Length > 500) throw new InvalidOperationException($"Object '{spec.Name}' needs text of at most 500 characters.");
                break;
            case "barcode":
                ValidateBarcode(spec.Variant, spec.Value);
                break;
            case "qr":
                if (string.IsNullOrEmpty(spec.Value) || spec.Value.Length > 512 || spec.Value.Any(char.IsControl))
                    throw new InvalidOperationException("QR data must contain 1–512 characters without control characters.");
                break;
            case "line":
                if (spec.Variant is not null && (!Enum.TryParse<ELineType>(spec.Variant, out var line) || !Enum.IsDefined(line)))
                    throw new InvalidOperationException("Unsupported line type.");
                break;
            case "shape":
                if (spec.Variant is not null && (!Enum.TryParse<EShapeType>(spec.Variant, out var shape) || !Enum.IsDefined(shape)))
                    throw new InvalidOperationException("Unsupported shape type.");
                break;
            case "date":
                _ = ParseDateTime(spec, out _);
                break;
            case "counter":
                if (spec.StartAt < 0 || spec.StartAt > 999999999 || spec.Increment < 1 || spec.Increment > 1000000 || spec.TextWidth is < 1 or > 9)
                    throw new InvalidOperationException("Counter start, increment or digit width is outside the supported range.");
                break;
            default:
                throw new InvalidOperationException($"Unsupported object type '{spec.Kind}'.");
        }
    }

    private static EBarcodeType BarcodeType(string? variant) => variant switch
    {
        null or "" or "Code128" => EBarcodeType.Code128Auto,
        "Code39" => EBarcodeType.Code39, "UpcE" => EBarcodeType.UpcE,
        "Ean13" => EBarcodeType.Ean13, "Ean8" => EBarcodeType.Ean8, "UpcA" => EBarcodeType.UpcA,
        _ => throw new InvalidOperationException("Unsupported barcode format.")
    };

    // Retail codes are supplied in full. The SDK accepts incorrect check digits,
    // so validate these independently before rendering or printing.
    private static void ValidateBarcode(string? variant, string? value)
    {
        var type = BarcodeType(variant);
        var text = value ?? "";
        switch (type)
        {
            case EBarcodeType.Code128Auto:
                if (text.Length is < 1 or > 100 || text.Any(ch => ch < 32 || ch > 126))
                    throw new InvalidOperationException("Code 128 requires 1–100 printable ASCII characters.");
                break;
            case EBarcodeType.Code39:
                if (text.Length is < 1 or > 100 || text.Any(ch => !(ch is >= 'A' and <= 'Z' or >= '0' and <= '9') && !" -.$/+%".Contains(ch)))
                    throw new InvalidOperationException("Code 39 requires 1–100 uppercase letters, digits or - . $ / + % and space.");
                break;
            case EBarcodeType.UpcE:
                if (!ValidUpcE(text)) throw new InvalidOperationException("UPC-E requires 8 digits including number system 0 or 1 and a valid check digit.");
                break;
            case EBarcodeType.Ean13:
            case EBarcodeType.Ean8:
            case EBarcodeType.UpcA:
                int length = type == EBarcodeType.Ean13 ? 13 : type == EBarcodeType.Ean8 ? 8 : 12;
                if (text.Length != length || !ValidRetailCheckDigit(text))
                    throw new InvalidOperationException("Enter the complete retail code with a valid check digit (EAN-13: 13, EAN-8: 8, UPC-A: 12 digits).");
                break;
            default: throw new InvalidOperationException("Unsupported barcode format.");
        }
    }

    private static bool ValidRetailCheckDigit(string value)
    {
        if (value.Length < 2 || value.Any(ch => ch is < '0' or > '9')) return false;
        int sum = 0;
        for (int i = value.Length - 2, position = 0; i >= 0; i--, position++)
            sum += (value[i] - '0') * (position % 2 == 0 ? 3 : 1);
        return (10 - sum % 10) % 10 == value[^1] - '0';
    }

    private static bool ValidUpcE(string value)
    {
        if (value.Length != 8 || value.Any(ch => ch is < '0' or > '9') || value[0] is not ('0' or '1')) return false;
        var d = value.Substring(1, 6);
        var body = d[5] switch
        {
            '0' or '1' or '2' => $"{d[0]}{d[1]}{d[5]}0000{d[2]}{d[3]}{d[4]}",
            '3' => $"{d[0]}{d[1]}{d[2]}00000{d[3]}{d[4]}",
            '4' => $"{d[0]}{d[1]}{d[2]}{d[3]}00000{d[4]}",
            _ => $"{d[0]}{d[1]}{d[2]}{d[3]}{d[4]}0000{d[5]}"
        };
        return ValidRetailCheckDigit(value[0] + body + value[7]);
    }

    private static DateTime ParseDateTime(FeatureObject spec, out ETimeFormat format)
    {
        var variant = string.IsNullOrEmpty(spec.Variant) ? "Date" : spec.Variant;
        string value;
        string pattern;
        switch (variant)
        {
            case "Date": format = ETimeFormat.Date; value = spec.Value ?? ""; pattern = "yyyy-MM-dd"; break;
            case "Time": format = ETimeFormat.Time; value = "2000-01-01T" + (spec.Value ?? ""); pattern = "yyyy-MM-dd'T'HH:mm"; break;
            case "DateAndTime": format = ETimeFormat.DateAndTime; value = spec.Value ?? ""; pattern = "yyyy-MM-dd'T'HH:mm"; break;
            default: throw new InvalidOperationException("Unsupported date/time format.");
        }
        if (!DateTime.TryParseExact(value, pattern, CultureInfo.InvariantCulture, DateTimeStyles.None, out var result))
            throw new InvalidOperationException("Enter a valid fixed date/time in the required ISO format.");
        return result;
    }
}

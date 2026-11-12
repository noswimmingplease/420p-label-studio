using System.Reflection;
using DYMO.LabelAPI;
using DYMO.LabelAPI.Interfaces;
using DymoSDK.Implementations;
using DymoSDK.Interfaces;

// The label is a fresh, per-request in-memory model. An error can occur after
// mutations; callers must discard the model on failure and never print it.
internal sealed record LayoutSpec(string Name, double Xmm, double Ymm, double WidthMm, double HeightMm, int Layer, bool AllowOverlap);
internal sealed record LayoutResolved(string Name, string Kind, double Xmm, double Ymm, double WidthMm, double HeightMm, int Layer, bool IsValid, bool InCanvas);

internal static class InteractiveLayout
{
    private const double Conversion = 25.4;
    private const double Tolerance = 0.15;

    private static ILabel<ILayoutManager> Model(IDymoLabel label)
    {
        var field = typeof(DymoLabel).GetField("_instanceLabel", BindingFlags.Instance | BindingFlags.NonPublic);
        return field?.GetValue(label) as ILabel<ILayoutManager> ?? throw new InvalidOperationException("SDK label model unavailable.");
    }

    // Return all objects, including pre-existing text/image/template objects.
    // Out-of-canvas bounds are returned with InCanvas=false so callers can let
    // the user repair them. Unbounded/non-finite geometry fails explicitly.
    internal static IReadOnlyList<LayoutResolved> Snapshot(IDymoLabel label)
    {
        var model = Model(label);
        if (label.GetPreviewLabel() is not { Length: > 0 }) throw new InvalidOperationException("The label cannot be rendered.");
        var bounds = model.LayoutBounds;
        if (!float.IsFinite(bounds.Size.Width) || !float.IsFinite(bounds.Size.Height) || bounds.Size.Width <= 0 || bounds.Size.Height <= 0)
            throw new InvalidOperationException("The SDK did not provide valid label bounds.");
        var output = new List<LayoutResolved>();
        var layer = 0;
        foreach (var obj in model.LayoutManager.LabelObjects)
        {
            if (layer >= 64) throw new InvalidOperationException("A layout can contain at most 64 objects.");
            if (string.IsNullOrWhiteSpace(obj.Name)) throw new InvalidOperationException("The SDK returned an object without a name.");
            var rect = obj.Layout.Rect;
            var x = (rect.Location.X - bounds.Location.X) * Conversion;
            var y = (rect.Location.Y - bounds.Location.Y) * Conversion;
            var width = rect.Size.Width * Conversion;
            var height = rect.Size.Height * Conversion;
            if (!double.IsFinite(x) || !double.IsFinite(y) || !double.IsFinite(width) || !double.IsFinite(height) ||
                Math.Abs(x) > 10000 || Math.Abs(y) > 100 || width <= 0 || width > 10000 || height <= 0 || height > 100)
                throw new InvalidOperationException($"DYMO returned invalid geometry for '{obj.Name}'.");
            var inCanvas = x >= -Tolerance && y >= -Tolerance &&
                x + width <= bounds.Size.Width * Conversion + Tolerance && y + height <= bounds.Size.Height * Conversion + Tolerance;
            output.Add(new LayoutResolved(obj.Name, obj.GetType().Name, x, y, width, height, layer++, obj.IsValidRender, inCanvas));
        }
        return output;
    }

    // The complete set of objects is required so layer indices are unambiguous.
    // Overlap is permitted only when both objects explicitly opt in.
    internal static IReadOnlyList<LayoutResolved> Apply(IDymoLabel label, IReadOnlyList<LayoutSpec> edits, string? emptyPrimaryText = null)
    {
        if (edits.Count is < 1 or > 64) throw new InvalidOperationException("A layout needs 1–64 objects.");
        if (edits.Any(e => e is null || string.IsNullOrWhiteSpace(e.Name))) throw new InvalidOperationException("Every layout object needs a name.");
        var model = Model(label);
        if (label.GetPreviewLabel() is not { Length: > 0 }) throw new InvalidOperationException("Existing label cannot be rendered.");
        var manager = model.LayoutManager;
        var objects = manager.LabelObjects.ToList();
        if (objects.Count != edits.Count) throw new InvalidOperationException("The object list changed after manual layout was enabled. Open Object layout, unlock it and reset to automatic layout before generating a new preview.");
        var byName = objects.ToDictionary(o => o.Name, StringComparer.OrdinalIgnoreCase);
        if (byName.Count != objects.Count || edits.Select(e => e.Name).Distinct(StringComparer.OrdinalIgnoreCase).Count() != edits.Count)
            throw new InvalidOperationException("Object names must be unique.");
        if (!edits.Select(e => e.Layer).Order().SequenceEqual(Enumerable.Range(0, edits.Count)))
            throw new InvalidOperationException("Layer indices must be contiguous and unique.");
        var initial = model.LayoutBounds;
        var initialHeight = initial.Size.Height * Conversion;
        if (!double.IsFinite(initialHeight) || initialHeight <= 0 || initialHeight > 100)
            throw new InvalidOperationException("The SDK did not provide a valid printable height.");
        foreach (var edit in edits)
        {
            if (!byName.ContainsKey(edit.Name)) throw new InvalidOperationException($"Object '{edit.Name}' is no longer in the label. Open Object layout, unlock it and reset to automatic layout before generating a new preview.");
            if (!double.IsFinite(edit.Xmm) || !double.IsFinite(edit.Ymm) || !double.IsFinite(edit.WidthMm) || !double.IsFinite(edit.HeightMm) ||
                edit.Xmm < 0 || edit.Ymm < 0 || edit.WidthMm <= 0 || edit.HeightMm <= 0 || edit.Xmm + edit.WidthMm > 2000 ||
                edit.Ymm + edit.HeightMm > initialHeight + Tolerance)
                throw new InvalidOperationException($"Invalid requested bounds for '{edit.Name}'.");
        }
        // A variable-length tape's auto-sized SDK model initially fits the
        // automatic layout, not necessarily the user's retained coordinates.
        // Expand only the printable width and preserve the origin and height;
        // fixed-length labels must keep their existing length and fail closed.
        var requiredWidth = edits.Max(edit => edit.Xmm + edit.WidthMm);
        var printableWidth = initial.Size.Width * Conversion;
        if (!double.IsFinite(printableWidth) || printableWidth <= 0 || printableWidth > 2000)
            throw new InvalidOperationException("The SDK did not provide a valid printable length.");
        if (requiredWidth > printableWidth + Tolerance && !model.HasFixedLength && manager.LengthBehavior == ELabelLengthBehavior.VariableLength)
        {
            var targetWidth = Math.Min(2000.0, requiredWidth + 0.30);
            var layoutBounds = model.GetType().GetProperty("LayoutBounds", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic);
            var setter = layoutBounds?.GetSetMethod(true) ?? throw new InvalidOperationException("This SDK cannot expand the variable-length label.");
            var expanded = new DYMORect(initial.Location, new DYMOSize((float)(targetWidth / Conversion), initial.Size.Height));
            setter.Invoke(model, new object[] { expanded });
            initial = model.LayoutBounds;
            var actualWidth = initial.Size.Width * Conversion;
            var tapeLength = model.LabelSize.Width * Conversion;
            if (!double.IsFinite(actualWidth) || actualWidth + Tolerance < requiredWidth ||
                !double.IsFinite(tapeLength) || tapeLength > 2000)
                throw new InvalidOperationException("The requested layout exceeds the supported label length.");
        }
        foreach (var edit in edits)
        {
            byName[edit.Name].Layout.ResetRect(
                new DYMOPoint(initial.Location.X + (float)(edit.Xmm / Conversion), initial.Location.Y + (float)(edit.Ymm / Conversion)),
                new DYMOSize((float)(edit.WidthMm / Conversion), (float)(edit.HeightMm / Conversion)));
        }
        foreach (var edit in edits.OrderBy(e => e.Layer)) manager.BringToFront(byName[edit.Name]);
        if (label.GetPreviewLabel() is not { Length: > 0 } || !manager.IsValidRender)
            throw new InvalidOperationException("The requested layout could not be rendered.");
        if (!manager.LabelObjects.Select(o => o.Name).SequenceEqual(edits.OrderBy(e => e.Layer).Select(e => e.Name), StringComparer.OrdinalIgnoreCase))
            throw new InvalidOperationException("DYMO did not preserve the requested layer order.");
        var actual = Snapshot(label);
        if (!actual.Select(o => o.Name).SequenceEqual(edits.OrderBy(e => e.Layer).Select(e => e.Name), StringComparer.OrdinalIgnoreCase))
            throw new InvalidOperationException("The SDK changed the requested layer order during the final render.");
        var resolvedByName = actual.ToDictionary(o => o.Name, StringComparer.OrdinalIgnoreCase);
        foreach (var obj in actual)
        {
            if (!obj.IsValid || !obj.InCanvas) throw new InvalidOperationException($"DYMO placed '{obj.Name}' outside the printable canvas or could not render it.");
        }
        for (var i = 0; i < edits.Count; i++) for (var j = i + 1; j < edits.Count; j++)
        {
            if (emptyPrimaryText is not null && (string.Equals(edits[i].Name, emptyPrimaryText, StringComparison.OrdinalIgnoreCase) || string.Equals(edits[j].Name, emptyPrimaryText, StringComparison.OrdinalIgnoreCase))) continue;
            var a = resolvedByName[edits[i].Name]; var b = resolvedByName[edits[j].Name];
            var overlapX = Math.Min(a.Xmm + a.WidthMm, b.Xmm + b.WidthMm) - Math.Max(a.Xmm, b.Xmm);
            var overlapY = Math.Min(a.Ymm + a.HeightMm, b.Ymm + b.HeightMm) - Math.Max(a.Ymm, b.Ymm);
            if (overlapX > Tolerance && overlapY > Tolerance && !(edits[i].AllowOverlap && edits[j].AllowOverlap))
                throw new InvalidOperationException($"'{a.Name}' overlaps '{b.Name}' without explicit permission.");
        }
        return actual;
    }
}

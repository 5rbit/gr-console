param([string]$SlideXml, [string]$OutCsv)
# Dump every shape with absolute slide coordinates (EMU -> pt), resolving group transforms.
$ErrorActionPreference = 'Stop'
$x = New-Object System.Xml.XmlDocument
$x.Load($SlideXml)
$ns = New-Object System.Xml.XmlNamespaceManager($x.NameTable)
$ns.AddNamespace('p', 'http://schemas.openxmlformats.org/presentationml/2006/main')
$ns.AddNamespace('a', 'http://schemas.openxmlformats.org/drawingml/2006/main')
$EMU = 12700.0
$rows = New-Object System.Collections.Generic.List[object]

function Get-Xfrm($node) {
  $xf = $node.SelectSingleNode('./p:spPr/a:xfrm | ./p:grpSpPr/a:xfrm', $ns)
  if (-not $xf) { return $null }
  $off = $xf.SelectSingleNode('a:off', $ns); $ext = $xf.SelectSingleNode('a:ext', $ns)
  $o = [ordered]@{ x = [double]$off.x; y = [double]$off.y; cx = [double]$ext.cx; cy = [double]$ext.cy; rot = 0.0; flipH = $false; flipV = $false }
  if ($xf.rot) { $o.rot = [double]$xf.rot / 60000.0 }
  if ($xf.flipH -eq '1') { $o.flipH = $true }
  if ($xf.flipV -eq '1') { $o.flipV = $true }
  $choff = $xf.SelectSingleNode('a:chOff', $ns); $chext = $xf.SelectSingleNode('a:chExt', $ns)
  if ($choff) { $o.chx = [double]$choff.x; $o.chy = [double]$choff.y; $o.chcx = [double]$chext.cx; $o.chcy = [double]$chext.cy }
  return $o
}

# transform: abs = (local - chOff) * scale + off, composed down the group stack
function Walk($parent, $tx) {
  foreach ($n in $parent.ChildNodes) {
    if ($n.LocalName -eq 'grpSp') {
      $g = Get-Xfrm $n
      $sx = if ($g.chcx) { $g.cx / $g.chcx } else { 1 }
      $sy = if ($g.chcy) { $g.cy / $g.chcy } else { 1 }
      $inner = @{
        sx = $tx.sx * $sx; sy = $tx.sy * $sy
        ox = $tx.ox + $tx.sx * ($g.x - $g.chx * $sx)
        oy = $tx.oy + $tx.sy * ($g.y - $g.chy * $sy)
      }
      Walk $n $inner
    } elseif ($n.LocalName -in @('sp', 'cxnSp', 'pic', 'graphicFrame')) {
      $f = Get-Xfrm $n
      if (-not $f) {
        $gx = $n.SelectSingleNode('./p:xfrm', $ns)
        if ($gx) { $f = @{ x = [double]$gx.SelectSingleNode('a:off', $ns).x; y = [double]$gx.SelectSingleNode('a:off', $ns).y; cx = [double]$gx.SelectSingleNode('a:ext', $ns).cx; cy = [double]$gx.SelectSingleNode('a:ext', $ns).cy; rot = 0 } } else { continue }
      }
      $geom = $n.SelectSingleNode('.//a:prstGeom', $ns)
      $name = $n.SelectSingleNode('./*/p:cNvPr', $ns).name
      $text = (($n.SelectNodes('.//a:t', $ns) | ForEach-Object { $_.InnerText }) -join ' ').Trim()
      $ax = $tx.ox + $tx.sx * $f.x; $ay = $tx.oy + $tx.sy * $f.y
      $w = $tx.sx * $f.cx; $h = $tx.sy * $f.cy
      $rows.Add([pscustomobject]@{
        kind = $n.LocalName; geom = if ($geom) { $geom.prst } else { '' }; name = $name
        cx_pt = [math]::Round(($ax + $w / 2) / $EMU, 2); cy_pt = [math]::Round(($ay + $h / 2) / $EMU, 2)
        w_pt = [math]::Round($w / $EMU, 2); h_pt = [math]::Round($h / $EMU, 2); rot = $f.rot; text = $text
      })
    }
  }
}
$tree = $x.SelectSingleNode('//p:cSld/p:spTree', $ns)
Walk $tree @{ sx = 1.0; sy = 1.0; ox = 0.0; oy = 0.0 }
$rows | Export-Csv -NoTypeInformation -Encoding UTF8 $OutCsv
"shapes=$($rows.Count) ellipses=$(@($rows | Where-Object geom -eq 'ellipse').Count)"

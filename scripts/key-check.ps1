# Shows which key Windows sees for each key press, in a small window: the virtual key and, for
# Shift, Ctrl, Alt and the Windows key, which side. For finding out what a key without a keycap
# (한/영, 한자) or a Mac keyboard's ⌘/⌥ sends. Close the window to stop.
#
#   powershell -ExecutionPolicy Bypass -File scripts\key-check.ps1
#
# Switch to another input method (Win+Space) before: an active input method may take keys like
# 한/영 before the window sees them.

Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace Nunchi -Name Keys -MemberDefinition @'
[DllImport("user32.dll")] public static extern short GetKeyState(int key);
'@

function Side([int]$left, [int]$right, [string]$name) {
    $l = [Nunchi.Keys]::GetKeyState($left) -lt 0
    $r = [Nunchi.Keys]::GetKeyState($right) -lt 0
    if ($r -and -not $l) { "right $name" } elseif ($l -and -not $r) { "left $name" } else { $name }
}

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Nunchi key check'
$form.Width = 520
$form.Height = 360
$form.KeyPreview = $true
$list = New-Object System.Windows.Forms.ListBox
$list.Dock = 'Fill'
$list.Font = New-Object System.Drawing.Font('Consolas', 12)
$form.Controls.Add($list)
$list.Items.Add('Press a key...') | Out-Null

$form.Add_KeyDown({
    param($sender, $e)
    $vk = [int]$e.KeyCode
    $what = switch ($vk) {
        0x10 { Side 0xA0 0xA1 'Shift' }
        0x11 { Side 0xA2 0xA3 'Ctrl' }
        0x12 { Side 0xA4 0xA5 'Alt' }
        0x5B { 'left Windows key' }
        0x5C { 'right Windows key' }
        0x15 { '한/영 key (VK_HANGUL)' }
        0x19 { '한자 key (VK_HANJA)' }
        0xE5 { 'taken by the input method (VK_PROCESSKEY)' }
        default { "$($e.KeyCode)" }
    }
    $list.Items.Insert(0, ('{0,-34} 0x{1:X2}' -f $what, $vk))
    $e.Handled = $true
    $e.SuppressKeyPress = $true
})

[System.Windows.Forms.Application]::Run($form)

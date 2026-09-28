//! The window's markup and its named parts.

use windows_core::Result;

use crate::bindings::*;
use crate::xaml;

const WINDOW_XAML: &str = r#"
<Grid {ns}>
  <Grid.Resources>
    <ResourceDictionary>
    <!-- The chrome is transparent over the window's backdrop; the selected tab is only a tint. -->
    <ResourceDictionary.ThemeDictionaries>
      <ResourceDictionary x:Key="Light">
        <StaticResource x:Key="TabViewItemHeaderBackgroundSelected" ResourceKey="SubtleFillColorSecondaryBrush"/>
      </ResourceDictionary>
      <ResourceDictionary x:Key="Default">
        <StaticResource x:Key="TabViewItemHeaderBackgroundSelected" ResourceKey="SubtleFillColorSecondaryBrush"/>
      </ResourceDictionary>
    </ResourceDictionary.ThemeDictionaries>
    <Style x:Key="ToolbarButton" TargetType="Button" BasedOn="{StaticResource DefaultButtonStyle}">
      <Setter Property="Background" Value="Transparent"/>
      <Setter Property="BorderThickness" Value="0"/>
      <Setter Property="Padding" Value="0"/>
      <Setter Property="Width" Value="36"/>
      <Setter Property="Height" Value="32"/>
    </Style>
    <Style x:Key="ToolbarToggle" TargetType="ToggleButton" BasedOn="{StaticResource DefaultToggleButtonStyle}">
      <Setter Property="Background" Value="Transparent"/>
      <Setter Property="BorderThickness" Value="0"/>
      <Setter Property="Padding" Value="0"/>
      <Setter Property="Width" Value="36"/>
      <Setter Property="Height" Value="32"/>
    </Style>
    <StaticResource x:Key="ButtonBackgroundPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ButtonBackgroundPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ButtonBackgroundDisabled" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushPointerOver" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushPressed" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushDisabled" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundChecked" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundCheckedPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundCheckedPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundChecked" ResourceKey="AccentTextFillColorPrimaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundCheckedPointerOver" ResourceKey="AccentTextFillColorPrimaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundCheckedPressed" ResourceKey="AccentTextFillColorSecondaryBrush"/>
    </ResourceDictionary>
  </Grid.Resources>
  <Grid.RowDefinitions>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="*"/>
  </Grid.RowDefinitions>

  <TabView x:Name="Tabs" TabWidthMode="Equal" IsAddTabButtonVisible="True" Visibility="Collapsed"
           CanReorderTabs="True" CanDragTabs="True" AllowDropTabs="True" VerticalAlignment="Bottom">
    <TabView.TabStripHeader>
      <Grid Width="8"/>
    </TabView.TabStripHeader>
    <TabView.TabStripFooter>
      <Grid x:Name="DragRegion" Background="Transparent" MinWidth="188"/>
    </TabView.TabStripFooter>
  </TabView>

  <Grid x:Name="Toolbar" Grid.Row="1" Padding="6,4,0,4" ColumnSpacing="2"
        Background="Transparent">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="*"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
    </Grid.ColumnDefinitions>
    <Button x:Name="Back" Style="{StaticResource ToolbarButton}" IsEnabled="False"
            ToolTipService.ToolTip="Back (Alt+Left)" AutomationProperties.Name="Back">
      <FontIcon Glyph="&#xE72B;" FontSize="16"/>
    </Button>
    <Button x:Name="Forward" Grid.Column="1" Style="{StaticResource ToolbarButton}" IsEnabled="False"
            ToolTipService.ToolTip="Forward (Alt+Right)" AutomationProperties.Name="Forward">
      <FontIcon Glyph="&#xE72A;" FontSize="16"/>
    </Button>
    <Button x:Name="Reload" Grid.Column="2" Style="{StaticResource ToolbarButton}"
            ToolTipService.ToolTip="Refresh (Ctrl+R)" AutomationProperties.Name="Refresh">
      <FontIcon x:Name="ReloadGlyph" Glyph="&#xE72C;" FontSize="16"/>
    </Button>
    <AutoSuggestBox x:Name="Address" Grid.Column="3" Margin="6,0" VerticalAlignment="Center"
                    PlaceholderText="Search or enter web address" UpdateTextOnSelect="False"
                    AutomationProperties.Name="Address and search bar"/>
    <ToggleButton x:Name="Star" Grid.Column="4" Style="{StaticResource ToolbarToggle}"
                  ToolTipService.ToolTip="Bookmark this page (Ctrl+D)" AutomationProperties.Name="Bookmark this page">
      <FontIcon x:Name="StarGlyph" Glyph="&#xE734;" FontSize="16"/>
    </ToggleButton>
    <StackPanel x:Name="ExtensionActions" Grid.Column="5" Orientation="Horizontal" Spacing="2"/>
    <Button x:Name="More" Grid.Column="6" Margin="0,0,6,0" Style="{StaticResource ToolbarButton}"
            ToolTipService.ToolTip="Settings and more" AutomationProperties.Name="Settings and more">
      <FontIcon Glyph="&#xE712;" FontSize="16"/>
      <Button.Flyout>
        <MenuFlyout Placement="BottomEdgeAlignedRight">
          {acrylic_menu}
          <MenuFlyoutItem x:Name="MenuNewTab" Text="New tab" KeyboardAcceleratorTextOverride="Ctrl+T">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xECCD;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuNewWindow" Text="New window" KeyboardAcceleratorTextOverride="Ctrl+N">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE78B;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutSeparator/>
          <MenuFlyoutItem x:Name="MenuBookmarks" Text="Bookmarks" KeyboardAcceleratorTextOverride="Ctrl+Shift+O">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE728;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuHistory" Text="History" KeyboardAcceleratorTextOverride="Ctrl+H">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE81C;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuExtensions" Text="Extensions">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xEA86;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutSeparator/>
          <MenuFlyoutItem x:Name="MenuSettings" Text="Settings">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE713;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuAbout" Text="About Vsesvit">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE946;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
        </MenuFlyout>
      </Button.Flyout>
    </Button>
    <Grid x:Name="ToolbarDrag" Grid.Column="7" Width="196" Background="Transparent" Visibility="Collapsed"/>
  </Grid>

  <Grid x:Name="BookmarksBar" Grid.Row="2" Height="30" Padding="8,0,8,2" Background="Transparent">
    <!-- Scrolls sideways (the mouse wheel too) instead of clipping when the items do not fit. -->
    <ScrollViewer x:Name="BookmarkScroller" HorizontalScrollMode="Enabled" HorizontalScrollBarVisibility="Hidden"
                  VerticalScrollMode="Disabled" VerticalScrollBarVisibility="Disabled">
      <StackPanel x:Name="BookmarkItems" Orientation="Horizontal" Spacing="1" VerticalAlignment="Center"/>
    </ScrollViewer>
    <TextBlock x:Name="BookmarksHint" Margin="6,0" VerticalAlignment="Center"
               Style="{StaticResource CaptionTextBlockStyle}"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"
               Text="For quick access, place your bookmarks here on the bookmarks bar."/>
  </Grid>

  <InfoBar x:Name="UpdateBar" Grid.Row="3" IsOpen="False" CornerRadius="0" BorderThickness="0,1,0,0">
    <InfoBar.ActionButton>
      <Button x:Name="UpdateAction"/>
    </InfoBar.ActionButton>
  </InfoBar>

  <Grid Grid.Row="4">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="*"/>
      <ColumnDefinition Width="Auto"/>
    </Grid.ColumnDefinitions>
    <Grid x:Name="LeftHost" Visibility="Collapsed"
          BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}" BorderThickness="0,1,1,0"/>
    <Grid x:Name="Pages" Grid.Column="1"
          Background="{ThemeResource SolidBackgroundFillColorTertiaryBrush}"
          BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}" BorderThickness="0,1,0,0"/>
    <Grid x:Name="RightHost" Grid.Column="2" Visibility="Collapsed"
          BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}" BorderThickness="1,1,0,0"/>
  </Grid>

  <Grid x:Name="Overlay" Grid.RowSpan="5" Visibility="Collapsed"
        Background="{ThemeResource SmokeFillColorDefaultBrush}">
    <Border HorizontalAlignment="Center" VerticalAlignment="Center" Padding="24" CornerRadius="8"
            Background="{ThemeResource ContentDialogBackground}"
            BorderBrush="{ThemeResource ContentDialogBorderBrush}" BorderThickness="1">
      <StackPanel Spacing="12">
        <TextBlock x:Name="OverlayTitle" Style="{StaticResource SubtitleTextBlockStyle}"/>
        <Grid x:Name="OverlayBody"/>
      </StackPanel>
    </Border>
  </Grid>
</Grid>"#;

/// Named parts of `WINDOW_XAML`.
pub(super) struct Chrome {
    pub(super) root: FrameworkElement,
    pub(super) tab_view: TabView,
    pub(super) drag_region: UIElement,
    pub(super) toolbar: UIElement,
    pub(super) toolbar_drag: UIElement,
    pub(super) back: Control,
    pub(super) forward: Control,
    pub(super) reload: Button,
    pub(super) reload_glyph: FontIcon,
    pub(super) address: AutoSuggestBox,
    pub(super) star: ToggleButton,
    pub(super) star_glyph: FontIcon,
    pub(super) extension_actions: Panel,
    pub(super) bookmarks_bar: UIElement,
    pub(super) bookmark_items: Panel,
    pub(super) bookmarks_hint: UIElement,
    pub(super) update_bar: InfoBar,
    pub(super) update_action: Button,
    pub(super) left_host: Panel,
    pub(super) pages: Panel,
    pub(super) right_host: Panel,
    pub(super) overlay: UIElement,
    pub(super) overlay_title: TextBlock,
    pub(super) overlay_body: Panel,
}

impl Chrome {
    pub(super) fn load() -> Result<Self> {
        let root: FrameworkElement = xaml::load(WINDOW_XAML)?;
        Ok(Self {
            tab_view: xaml::find(&root, "Tabs")?,
            drag_region: xaml::find(&root, "DragRegion")?,
            toolbar: xaml::find(&root, "Toolbar")?,
            toolbar_drag: xaml::find(&root, "ToolbarDrag")?,
            back: xaml::find(&root, "Back")?,
            forward: xaml::find(&root, "Forward")?,
            reload: xaml::find(&root, "Reload")?,
            reload_glyph: xaml::find(&root, "ReloadGlyph")?,
            address: xaml::find(&root, "Address")?,
            star: xaml::find(&root, "Star")?,
            star_glyph: xaml::find(&root, "StarGlyph")?,
            extension_actions: xaml::find(&root, "ExtensionActions")?,
            bookmarks_bar: xaml::find(&root, "BookmarksBar")?,
            bookmark_items: xaml::find(&root, "BookmarkItems")?,
            bookmarks_hint: xaml::find(&root, "BookmarksHint")?,
            update_bar: xaml::find(&root, "UpdateBar")?,
            update_action: xaml::find(&root, "UpdateAction")?,
            left_host: xaml::find(&root, "LeftHost")?,
            pages: xaml::find(&root, "Pages")?,
            right_host: xaml::find(&root, "RightHost")?,
            overlay: xaml::find(&root, "Overlay")?,
            overlay_title: xaml::find(&root, "OverlayTitle")?,
            overlay_body: xaml::find(&root, "OverlayBody")?,
            root,
        })
    }
}

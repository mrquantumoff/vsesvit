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

  <TabView x:Name="Tabs" TabWidthMode="SizeToContent" IsAddTabButtonVisible="True" Visibility="Collapsed"
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
    <StackPanel Grid.Column="2" Orientation="Horizontal" Spacing="2">
      <Button x:Name="Reload" Style="{StaticResource ToolbarButton}"
              ToolTipService.ToolTip="Refresh (Ctrl+R)" AutomationProperties.Name="Refresh">
        <FontIcon x:Name="ReloadGlyph" Glyph="&#xE72C;" FontSize="16"/>
      </Button>
      <Button x:Name="Home" Style="{StaticResource ToolbarButton}" Visibility="Collapsed"
              ToolTipService.ToolTip="Home" AutomationProperties.Name="Home">
        <FontIcon Glyph="&#xE80F;" FontSize="16"/>
      </Button>
    </StackPanel>
    <!-- The address pill, as in Brave: the page's security at its start, the address centered
         in it (at the start while editing), the star at its end. -->
    <Grid x:Name="AddressPill" Grid.Column="3" Margin="12,0" Height="32" VerticalAlignment="Center"
          CornerRadius="8" Background="{ThemeResource ControlFillColorDefaultBrush}"
          BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}" BorderThickness="1">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="Auto"/>
        <ColumnDefinition Width="*"/>
        <ColumnDefinition Width="Auto"/>
        <ColumnDefinition Width="Auto"/>
        <ColumnDefinition Width="Auto"/>
      </Grid.ColumnDefinitions>
      <Button x:Name="SiteButton" Margin="4,0,0,0" Width="30" Height="26" Padding="0" CornerRadius="6"
              Background="Transparent" BorderThickness="0" AutomationProperties.Name="View site information">
        <FontIcon x:Name="SiteIcon" FontSize="14" Glyph="&#xE721;"
                  Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
      </Button>
      <AutoSuggestBox x:Name="Address" Grid.Column="1" VerticalAlignment="Center"
                      PlaceholderText="Search or enter web address" UpdateTextOnSelect="False"
                      AutomationProperties.Name="Address and search bar">
        <AutoSuggestBox.Resources>
          <SolidColorBrush x:Key="TextControlBackground" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlBackgroundPointerOver" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlBackgroundFocused" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlBorderBrush" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlBorderBrushPointerOver" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlBorderBrushFocused" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlElevationBorderBrush" Color="Transparent"/>
          <SolidColorBrush x:Key="TextControlElevationBorderFocusedBrush" Color="Transparent"/>
        </AutoSuggestBox.Resources>
        <AutoSuggestBox.TextBoxStyle>
          <Style TargetType="TextBox" BasedOn="{StaticResource AutoSuggestBoxTextBoxStyle}">
            <Setter Property="TextAlignment" Value="Center"/>
          </Style>
        </AutoSuggestBox.TextBoxStyle>
      </AutoSuggestBox>
      <!-- The page's zoom when it is not 100%, as in Chrome; it opens the zoom bubble. The bubble
           takes no focus, so its buttons zoom the page that has it. -->
      <Button x:Name="ZoomChip" Grid.Column="2" Height="24" Padding="8,0" Margin="0,0,2,0"
              CornerRadius="12" FontSize="12" Visibility="Collapsed" AllowFocusOnInteraction="False"
              Background="{ThemeResource SubtleFillColorSecondaryBrush}" BorderThickness="0"
              ToolTipService.ToolTip="Zoom" AutomationProperties.Name="Zoom">
        <TextBlock x:Name="ZoomChipText" Text="100%"/>
        <Button.Flyout>
          <Flyout x:Name="ZoomBubble" Placement="BottomEdgeAlignedRight" ShowMode="Transient">
            <StackPanel Orientation="Horizontal" Spacing="8">
              <TextBlock Text="Zoom:" VerticalAlignment="Center"/>
              <TextBlock x:Name="ZoomLevel" Text="100%" MinWidth="44" VerticalAlignment="Center"/>
              <Button x:Name="ZoomOut" Width="36" AllowFocusOnInteraction="False"
                      ToolTipService.ToolTip="Zoom out (Ctrl+minus)" AutomationProperties.Name="Zoom out">
                <FontIcon Glyph="&#xE738;" FontSize="12"/>
              </Button>
              <Button x:Name="ZoomIn" Width="36" AllowFocusOnInteraction="False"
                      ToolTipService.ToolTip="Zoom in (Ctrl+plus)" AutomationProperties.Name="Zoom in">
                <FontIcon Glyph="&#xE710;" FontSize="12"/>
              </Button>
              <Button x:Name="ZoomReset" Content="Reset" AllowFocusOnInteraction="False"
                      ToolTipService.ToolTip="Reset to the default (Ctrl+0)"/>
            </StackPanel>
          </Flyout>
        </Button.Flyout>
      </Button>
      <Button x:Name="CopyLink" Grid.Column="3" Style="{StaticResource ToolbarButton}" Width="32" Height="26"
              Margin="0,0,2,0" CornerRadius="6" Visibility="Collapsed"
              ToolTipService.ToolTip="Copy link without trackers (Ctrl+Shift+C)" AutomationProperties.Name="Copy link">
        <FontIcon x:Name="CopyLinkGlyph" Glyph="&#xE8C8;" FontSize="14"/>
      </Button>
      <ToggleButton x:Name="Star" Grid.Column="4" Style="{StaticResource ToolbarToggle}" Width="32" Height="26"
                    Margin="0,0,2,0" CornerRadius="6"
                    ToolTipService.ToolTip="Bookmark this page (Ctrl+D)" AutomationProperties.Name="Bookmark this page">
        <FontIcon x:Name="StarGlyph" Glyph="&#xE734;" FontSize="14"/>
      </ToggleButton>
      <Border x:Name="AddressFocusRing" Grid.ColumnSpan="5" CornerRadius="8" BorderThickness="2" Margin="-1"
              BorderBrush="{ThemeResource AccentFillColorDefaultBrush}" IsHitTestVisible="False"
              Visibility="Collapsed"/>
    </Grid>
    <!-- The pinned extension actions, which drag to reorder, and the Extensions menu. -->
    <StackPanel x:Name="ExtensionActions" Grid.Column="4" Orientation="Horizontal" Spacing="2">
      <ListView x:Name="PinnedExtensions" SelectionMode="None" IsItemClickEnabled="True"
                CanDragItems="True" CanReorderItems="True" AllowDrop="True" VerticalAlignment="Center"
                ScrollViewer.HorizontalScrollMode="Disabled" ScrollViewer.HorizontalScrollBarVisibility="Disabled"
                ScrollViewer.VerticalScrollMode="Disabled" ScrollViewer.VerticalScrollBarVisibility="Disabled"
                AutomationProperties.Name="Pinned extensions">
        <ListView.ItemsPanel>
          <ItemsPanelTemplate><ItemsStackPanel Orientation="Horizontal"/></ItemsPanelTemplate>
        </ListView.ItemsPanel>
      </ListView>
      <Button x:Name="ExtensionsMenu" Style="{StaticResource ToolbarButton}" Visibility="Collapsed"
              ToolTipService.ToolTip="Extensions" AutomationProperties.Name="Extensions">
        <FontIcon Glyph="&#xEA86;" FontSize="16"/>
      </Button>
    </StackPanel>
    <Button x:Name="Downloads" Grid.Column="5" Style="{StaticResource ToolbarButton}" Visibility="Collapsed"
            ToolTipService.ToolTip="Downloads (Ctrl+J)" AutomationProperties.Name="Downloads">
      <Grid>
        <FontIcon Glyph="&#xE896;" FontSize="16"/>
        <ProgressRing x:Name="DownloadsBusy" Width="28" Height="28" MinWidth="28" MinHeight="28" IsActive="False"/>
      </Grid>
    </Button>
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
          <MenuFlyoutItem x:Name="MenuDownloads" Text="Downloads" KeyboardAcceleratorTextOverride="Ctrl+J">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE896;"/></MenuFlyoutItem.Icon>
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
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="*"/>
      <ColumnDefinition Width="Auto"/>
    </Grid.ColumnDefinitions>
    <!-- Items drag to reorder. The bar never scrolls: what does not fit whole is collapsed and
         listed by the chevron instead. -->
    <ListView x:Name="BookmarkItems" SelectionMode="None" IsItemClickEnabled="True"
              CanDragItems="True" CanReorderItems="True" AllowDrop="True"
              ScrollViewer.HorizontalScrollMode="Disabled" ScrollViewer.HorizontalScrollBarVisibility="Disabled"
              ScrollViewer.VerticalScrollMode="Disabled" ScrollViewer.VerticalScrollBarVisibility="Disabled"
              AutomationProperties.Name="Bookmarks bar">
      <ListView.ItemsPanel>
        <ItemsPanelTemplate><ItemsStackPanel Orientation="Horizontal"/></ItemsPanelTemplate>
      </ListView.ItemsPanel>
    </ListView>
    <Button x:Name="BookmarksOverflow" Grid.Column="1" Style="{StaticResource ToolbarButton}" Width="28" Height="24"
            Margin="2,0,0,0" CornerRadius="4" Visibility="Collapsed"
            ToolTipService.ToolTip="More bookmarks" AutomationProperties.Name="More bookmarks">
      <TextBlock Text="&#x00BB;" FontSize="16" Margin="0,-3,0,0"/>
    </Button>
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
    <!-- A tab's web view spans the three columns; in split view two tabs take the outer ones. -->
    <Grid x:Name="Pages" Grid.Column="1"
          Background="{ThemeResource SolidBackgroundFillColorTertiaryBrush}"
          BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}" BorderThickness="0,1,0,0">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="*"/>
        <ColumnDefinition Width="Auto"/>
        <ColumnDefinition Width="*"/>
      </Grid.ColumnDefinitions>
      <!-- Drags to share the width between the two pages. -->
      <Border x:Name="SplitDivider" Grid.Column="1" Width="8" Visibility="Collapsed" Background="Transparent"
              AutomationProperties.Name="Resize the split view">
        <Border Width="2" Background="{ThemeResource DividerStrokeColorDefaultBrush}"/>
      </Border>
    </Grid>
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
    pub(super) more: FrameworkElement,
    pub(super) toolbar_drag: UIElement,
    pub(super) back: Control,
    pub(super) forward: Control,
    pub(super) reload: Button,
    pub(super) reload_glyph: FontIcon,
    pub(super) home: Button,
    pub(super) address: AutoSuggestBox,
    pub(super) address_pill: FrameworkElement,
    pub(super) address_focus_ring: UIElement,
    pub(super) site_button: Button,
    pub(super) site_icon: FontIcon,
    pub(super) zoom_chip: Button,
    pub(super) zoom_chip_text: TextBlock,
    pub(super) zoom_bubble: Flyout,
    pub(super) zoom_level: TextBlock,
    pub(super) copy_link: Button,
    pub(super) copy_link_glyph: FontIcon,
    pub(super) star: ToggleButton,
    pub(super) star_glyph: FontIcon,
    pub(super) extension_actions: Panel,
    pub(super) pinned_extensions: ListView,
    pub(super) extensions_menu: Button,
    pub(super) downloads: Button,
    pub(super) downloads_busy: ProgressRing,
    pub(super) bookmarks_bar: FrameworkElement,
    pub(super) bookmark_items: ListView,
    pub(super) bookmarks_overflow: Button,
    pub(super) bookmarks_hint: UIElement,
    pub(super) update_bar: InfoBar,
    pub(super) update_action: Button,
    pub(super) left_host: Panel,
    pub(super) pages: Panel,
    pub(super) split_divider: UIElement,
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
            more: xaml::find(&root, "More")?,
            toolbar_drag: xaml::find(&root, "ToolbarDrag")?,
            back: xaml::find(&root, "Back")?,
            forward: xaml::find(&root, "Forward")?,
            reload: xaml::find(&root, "Reload")?,
            reload_glyph: xaml::find(&root, "ReloadGlyph")?,
            home: xaml::find(&root, "Home")?,
            address: xaml::find(&root, "Address")?,
            address_pill: xaml::find(&root, "AddressPill")?,
            address_focus_ring: xaml::find(&root, "AddressFocusRing")?,
            site_button: xaml::find(&root, "SiteButton")?,
            site_icon: xaml::find(&root, "SiteIcon")?,
            zoom_chip: xaml::find(&root, "ZoomChip")?,
            zoom_chip_text: xaml::find(&root, "ZoomChipText")?,
            zoom_bubble: xaml::find(&root, "ZoomBubble")?,
            zoom_level: xaml::find(&root, "ZoomLevel")?,
            copy_link: xaml::find(&root, "CopyLink")?,
            copy_link_glyph: xaml::find(&root, "CopyLinkGlyph")?,
            star: xaml::find(&root, "Star")?,
            star_glyph: xaml::find(&root, "StarGlyph")?,
            extension_actions: xaml::find(&root, "ExtensionActions")?,
            pinned_extensions: xaml::find(&root, "PinnedExtensions")?,
            extensions_menu: xaml::find(&root, "ExtensionsMenu")?,
            downloads: xaml::find(&root, "Downloads")?,
            downloads_busy: xaml::find(&root, "DownloadsBusy")?,
            bookmarks_bar: xaml::find(&root, "BookmarksBar")?,
            bookmark_items: xaml::find(&root, "BookmarkItems")?,
            bookmarks_overflow: xaml::find(&root, "BookmarksOverflow")?,
            bookmarks_hint: xaml::find(&root, "BookmarksHint")?,
            update_bar: xaml::find(&root, "UpdateBar")?,
            update_action: xaml::find(&root, "UpdateAction")?,
            left_host: xaml::find(&root, "LeftHost")?,
            pages: xaml::find(&root, "Pages")?,
            split_divider: xaml::find(&root, "SplitDivider")?,
            right_host: xaml::find(&root, "RightHost")?,
            overlay: xaml::find(&root, "Overlay")?,
            overlay_title: xaml::find(&root, "OverlayTitle")?,
            overlay_body: xaml::find(&root, "OverlayBody")?,
            root,
        })
    }
}

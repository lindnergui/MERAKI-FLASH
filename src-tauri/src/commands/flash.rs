use crate::flash::protocol::{StartFlashRequest, StartFlashResponse};
use crate::flash::{self, FlashManager};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn start_flash(
    app: AppHandle,
    manager: State<'_, FlashManager>,
    request: StartFlashRequest,
) -> Result<StartFlashResponse, String> {
    flash::start_flash(app, &manager, request).await
}

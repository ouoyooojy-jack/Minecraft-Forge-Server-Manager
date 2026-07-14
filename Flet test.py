import flet as ft

def main(page: ft.Page):
    page.title = "APP"
    page.window_width = 400
    page.window_height = 300
    
    my_text =ft.Text("Hello", size=20, weight="bold", color="blue")
    
    layout = ft.Column(
        controls=[my_text],
        alignment=ft.MainAxisAlignment.CENTER,
        horizontal_alignment=ft.CrossAxisAlignment.CENTER,
        expand=True
    )

    page.add(layout)
    
ft.run(main)
 